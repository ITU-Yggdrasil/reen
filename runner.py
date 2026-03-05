#!/usr/bin/env python3
"""
Model execution runner for the reen agent system.

This script receives agent execution requests via stdin and returns results via stdout.
It supports multiple LLM providers through a unified interface.
"""

import json
import sys
import os
import socket
from urllib.parse import urlparse
from typing import Dict, Any, Optional

# Load environment variables from .env file if it exists
try:
    from dotenv import load_dotenv
    # Load .env from the script's directory, resolving symlinks
    script_dir = os.path.dirname(os.path.realpath(__file__))
    dotenv_path = os.path.join(script_dir, '.env')
    load_dotenv(dotenv_path)
except ImportError:
    # dotenv not installed, continue without it
    pass

# Auto-detect and use venv if available
def ensure_venv():
    """Ensure we're using the project's virtual environment if it exists."""
    # Check if we're already in a venv
    if hasattr(sys, 'real_prefix') or (hasattr(sys, 'base_prefix') and sys.base_prefix != sys.prefix):
        # Already in a venv
        return
    
    # Look for project venv, resolving symlinks to find the real script location
    script_dir = os.path.dirname(os.path.realpath(__file__))
    venv_python = os.path.join(script_dir, '.venv', 'bin', 'python3')
    
    if os.path.exists(venv_python):
        # Re-execute with venv Python
        os.execv(venv_python, [venv_python] + sys.argv)

# Run venv check before anything else
ensure_venv()


def execute_with_anthropic(model: str, system_prompt: str) -> str:
    """Execute using Anthropic's Claude API."""
    try:
        import anthropic
    except ImportError:
        raise RuntimeError("anthropic package not installed. Run: pip install anthropic")

    api_key = os.environ.get("ANTHROPIC_API_KEY")
    if not api_key:
        raise RuntimeError("ANTHROPIC_API_KEY environment variable not set")

    client = anthropic.Anthropic(api_key=api_key)

    message = client.messages.create(
        model=model,
        max_tokens=8096,
        system=system_prompt,
        messages=[
            {"role": "user", "content": "Please complete the task described in the system prompt."}
        ]
    )

    return message.content[0].text


def execute_with_ollama(model: str, system_prompt: str) -> str:
    """Execute using Ollama's local API."""
    try:
        import ollama
    except ImportError:
        raise RuntimeError("ollama package not installed. Run: pip install ollama")

    # Set base URL if provided (for remote Ollama instances)
    # Default to localhost:11434 for local Ollama instances
    base_url = os.environ.get("OLLAMA_BASE_URL", "http://localhost:11434")
    client = ollama.Client(host=base_url)
    
    # Extract model name (handle format like "qwen2.5:7b" or "ollama:qwen2.5:7b")
    # Models in models.yml are like "qwen2.5:7b" which is the correct format for Ollama
    model_name = model
    
    # Remove "ollama:" prefix if present
    if model_name.startswith("ollama:"):
        model_name = model_name[7:]
    
    # The model name format "qwen2.5:7b" is correct for Ollama (model:tag)
    response = client.chat(
        model=model_name,
        messages=[
            {"role": "system", "content": system_prompt},
            {"role": "user", "content": "Please complete the task described in the system prompt."}
        ]
    )

    return response["message"]["content"]


def execute_with_openai(model: str, system_prompt: str) -> str:
    """Execute using OpenAI's API."""
    try:
        from openai import OpenAI
    except ImportError:
        raise RuntimeError("openai package not installed. Run: pip install openai")

    api_key = os.environ.get("OPENAI_API_KEY")
    if not api_key:
        raise RuntimeError("OPENAI_API_KEY environment variable not set")

    base_url = os.environ.get("OPENAI_BASE_URL")
    timeout = float(os.environ.get("OPENAI_TIMEOUT_SECONDS", "180"))
    max_retries = int(os.environ.get("OPENAI_MAX_RETRIES", "3"))
    client_kwargs: Dict[str, Any] = {
        "api_key": api_key,
        "timeout": timeout,
        "max_retries": max_retries,
    }
    if base_url:
        client_kwargs["base_url"] = base_url

    # Validate DNS resolution before request to produce actionable failures.
    endpoint = base_url if base_url else "https://api.openai.com/v1"
    parsed = urlparse(endpoint)
    host = parsed.hostname
    if host:
        try:
            socket.getaddrinfo(host, 443)
        except OSError as e:
            raise RuntimeError(f"DNS resolution failed for OpenAI host '{host}': {e}")

    client = OpenAI(**client_kwargs)

    response = client.chat.completions.create(
        model=model,
        messages=[
            {"role": "system", "content": system_prompt},
            {"role": "user", "content": "Please complete the task described in the system prompt."}
        ]
    )

    return response.choices[0].message.content


def determine_provider(model: str) -> str:
    """Determine which provider to use based on model name."""
    model_lower = model.lower()

    if any(x in model_lower for x in ["claude", "anthropic"]):
        return "anthropic"
    elif any(x in model_lower for x in ["ollama", "qwen", "llama", "mistral", "phi", "gemma", "codellama"]):
        return "ollama"
    elif any(x in model_lower for x in ["gpt", "openai", "o1", "o3"]):
        return "openai"
    else:
        # Default to Ollama for unknown models (local, no API key needed)
        return "ollama"


def execute_model(request: Dict[str, Any]) -> Dict[str, Any]:
    """Execute a model request and return the result."""
    try:
        model = request.get("model")
        system_prompt = request.get("system_prompt")

        if not model or not system_prompt:
            return {
                "success": False,
                "error": "Missing required fields: model and system_prompt"
            }

        provider = determine_provider(model)

        if provider == "anthropic":
            output = execute_with_anthropic(model, system_prompt)
        elif provider == "ollama":
            output = execute_with_ollama(model, system_prompt)
        elif provider == "openai":
            output = execute_with_openai(model, system_prompt)
        else:
            return {
                "success": False,
                "error": f"Unknown provider: {provider}"
            }

        return {
            "success": True,
            "output": output
        }

    except Exception as e:
        return {
            "success": False,
            "error": format_exception(e)
        }


def format_exception(exc: Exception) -> str:
    """Format exceptions with chained causes to keep root cause visible."""
    parts = [f"{type(exc).__name__}: {exc}"]
    seen = {id(exc)}

    current: Optional[BaseException] = exc
    depth = 0
    while current is not None and depth < 3:
        cause = current.__cause__ or current.__context__
        if cause is None or id(cause) in seen:
            break
        seen.add(id(cause))
        parts.append(f"caused by {type(cause).__name__}: {cause}")
        current = cause
        depth += 1

    return " | ".join(parts)


def main():
    """Main entry point - reads JSON from stdin, executes, writes JSON to stdout."""
    try:
        # Read the entire input
        input_data = sys.stdin.read()

        if not input_data.strip():
            response = {
                "success": False,
                "error": "No input provided"
            }
            print(json.dumps(response), flush=True)
            sys.exit(1)

        # Parse the JSON request
        request = json.loads(input_data)

        # Execute the request
        response = execute_model(request)

        # Write the response
        print(json.dumps(response), flush=True)

        # Exit with appropriate code
        sys.exit(0 if response.get("success") else 1)

    except json.JSONDecodeError as e:
        response = {
            "success": False,
            "error": f"Invalid JSON input: {e}"
        }
        print(json.dumps(response), flush=True)
        sys.exit(1)

    except Exception as e:
        response = {
            "success": False,
            "error": f"Unexpected error: {e}"
        }
        print(json.dumps(response), flush=True)
        sys.exit(1)


if __name__ == "__main__":
    main()
