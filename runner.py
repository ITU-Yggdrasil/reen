#!/usr/bin/env python3
"""
Model execution runner for the reen agent system.

This script receives agent execution requests via stdin and returns results via stdout.
It supports multiple LLM providers through a unified interface.
"""

import hashlib
import json
import sys
import os
import socket
from urllib.parse import urlparse
from typing import Dict, Any, Optional, Tuple

# Resolve the starting directory. When the script is embedded in the reen binary
# and written to a temp file, REEN_PROJECT_DIR tells us where to start looking.
_start_dir = os.environ.get("REEN_PROJECT_DIR") or os.path.dirname(os.path.realpath(__file__))


def _find_upwards(start: str, name: str) -> Optional[str]:
    """Walk from start up to the filesystem root looking for a file or directory."""
    current = os.path.abspath(start)
    while True:
        candidate = os.path.join(current, name)
        if os.path.exists(candidate):
            return candidate
        parent = os.path.dirname(current)
        if parent == current:
            return None
        current = parent


# Load environment variables from .env file if it exists
try:
    from dotenv import load_dotenv
    dotenv_path = _find_upwards(_start_dir, '.env')
    if dotenv_path:
        load_dotenv(dotenv_path)
except ImportError:
    # dotenv not installed, continue without it
    pass

# Auto-detect and use venv if available
def ensure_venv():
    """Ensure we're using the project's virtual environment if it exists."""
    if hasattr(sys, 'real_prefix') or (hasattr(sys, 'base_prefix') and sys.base_prefix != sys.prefix):
        return
    
    venv_python = _find_upwards(_start_dir, '.venv')
    if venv_python:
        venv_python = os.path.join(venv_python, 'bin', 'python3')
        if os.path.exists(venv_python):
            os.execv(venv_python, [venv_python] + sys.argv)

# Run venv check before anything else
ensure_venv()


def _resolve_max_output_tokens(
    request_value: Optional[int], env_var: str, default: int
) -> int:
    """Resolve max output tokens from request, env, or default."""
    if request_value is not None:
        return max(1, int(request_value))
    env_value = os.environ.get(env_var)
    if env_value:
        try:
            return max(1, int(env_value))
        except ValueError:
            pass
    return default


def execute_with_anthropic(
    model: str,
    system_content: str,
    user_content: str,
    max_output_tokens: Optional[int] = None,
) -> str:
    """Execute using Anthropic's Claude API with prompt caching."""
    try:
        import anthropic
    except ImportError:
        raise RuntimeError("anthropic package not installed. Run: pip install anthropic")

    api_key = os.environ.get("ANTHROPIC_API_KEY")
    if not api_key:
        raise RuntimeError("ANTHROPIC_API_KEY environment variable not set")

    client = anthropic.Anthropic(api_key=api_key)
    max_tokens = _resolve_max_output_tokens(
        max_output_tokens, "ANTHROPIC_MAX_OUTPUT_TOKENS", 8096
    )

    try:
        message = client.messages.create(
            model=model,
            max_tokens=max_tokens,
            cache_control={"type": "ephemeral"},
            system=system_content,
            messages=[{"role": "user", "content": user_content}],
        )
    except Exception as e:
        status_code = getattr(e, "status_code", None)
        detail = str(e)
        if status_code == 429 or "rate limit" in detail.lower():
            raise RuntimeError(f"Anthropic rate limit exceeded (429): {detail}") from e
        raise

    return message.content[0].text


def execute_with_ollama(
    model: str, system_content: str, user_content: str
) -> str:
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
            {"role": "system", "content": system_content},
            {"role": "user", "content": user_content},
        ],
    )

    first_output = response["message"]["content"]
    lower = first_output.lower()
    asks_for_prompt = (
        "provide me with the system prompt" in lower
        or "provide the system prompt" in lower
        or "provide me with the details of the task" in lower
        or "provide details of the task" in lower
    )
    if asks_for_prompt:
        fallback_response = client.chat(
            model=model_name,
            messages=[
                {
                    "role": "user",
                    "content": (
                        f"{system_content}\n\n{user_content}\n\n"
                        "Please complete the task described above. "
                        "Return only the final result."
                    ),
                }
            ],
        )
        return fallback_response["message"]["content"]

    return first_output


def _openai_prompt_cache_key(agent_name: str, static_prompt: str) -> str:
    """Generate stable prompt_cache_key for OpenAI cache routing."""
    short_hash = hashlib.sha256(static_prompt.encode()).hexdigest()[:16]
    return f"reen:{agent_name}:{short_hash}"


def _openai_supports_extended_cache(model: str) -> bool:
    """Check if model supports 24h prompt cache retention."""
    model_lower = model.lower()
    return any(
        x in model_lower
        for x in ["gpt-4.1", "gpt-5", "gpt-5.1", "gpt-5.2", "gpt-5.4", "gp5-5.1"]
    )


def execute_with_openai(
    model: str,
    system_content: str,
    user_content: str,
    agent_name: Optional[str] = None,
) -> str:
    """Execute using OpenAI's API with prompt caching."""
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

    create_kwargs: Dict[str, Any] = {
        "model": model,
        "messages": [
            {"role": "system", "content": system_content},
            {"role": "user", "content": user_content},
        ],
    }
    if agent_name and len(system_content) >= 256:
        create_kwargs["prompt_cache_key"] = _openai_prompt_cache_key(
            agent_name, system_content
        )
    if _openai_supports_extended_cache(model):
        create_kwargs["prompt_cache_retention"] = "24h"

    response = client.chat.completions.create(**create_kwargs)

    return response.choices[0].message.content


def execute_with_mistral(
    model: str, system_content: str, user_content: str
) -> str:
    """Execute using Mistral's API (OpenAI-compatible)."""
    try:
        from openai import OpenAI
    except ImportError:
        raise RuntimeError("openai package not installed. Run: pip install openai")

    api_key = os.environ.get("MISTRAL_API_KEY")
    if not api_key:
        raise RuntimeError("MISTRAL_API_KEY environment variable not set")

    base_url = os.environ.get("MISTRAL_BASE_URL", "https://api.mistral.ai/v1")
    timeout = float(os.environ.get("MISTRAL_TIMEOUT_SECONDS", "180"))
    max_retries = int(os.environ.get("MISTRAL_MAX_RETRIES", "3"))

    client = OpenAI(
        api_key=api_key,
        base_url=base_url,
        timeout=timeout,
        max_retries=max_retries,
    )

    response = client.chat.completions.create(
        model=model,
        messages=[
            {"role": "system", "content": system_content},
            {"role": "user", "content": user_content},
        ],
    )

    return response.choices[0].message.content


def determine_provider(model: str) -> tuple:
    """Determine which provider to use based on model name.

    Supports an explicit 'provider/model' prefix (e.g. 'mistral/codestral-latest').
    When no prefix is present, falls back to substring-based heuristics.

    Returns (provider, model_name).
    """
    if "/" in model:
        provider, model_name = model.split("/", 1)
        return provider.lower(), model_name

    model_lower = model.lower()

    if any(x in model_lower for x in ["claude", "anthropic"]):
        return "anthropic", model
    elif any(x in model_lower for x in ["ollama", "qwen", "llama", "mistral", "phi", "gemma", "codellama"]):
        return "ollama", model
    elif any(x in model_lower for x in ["gpt", "openai", "o1", "o3"]):
        return "openai", model
    else:
        return "ollama", model


def _normalize_request(
    request: Dict[str, Any]
) -> Tuple[str, str, Optional[str], Optional[int]]:
    """Normalize request to (system_content, user_content, agent_name, max_output_tokens).
    Supports split format (static_prompt + variable_prompt) and legacy (system_prompt).
    """
    model = request.get("model")
    static_prompt = request.get("static_prompt")
    variable_prompt = request.get("variable_prompt")
    system_prompt = request.get("system_prompt")
    agent_name = request.get("agent_name")
    max_output_tokens = request.get("max_output_tokens")

    if static_prompt is not None and variable_prompt is not None:
        return (static_prompt, variable_prompt, agent_name, max_output_tokens)
    if system_prompt is not None:
        return (
            system_prompt,
            "Please complete the task described in the system prompt.",
            agent_name,
            max_output_tokens,
        )
    return ("", "", agent_name, max_output_tokens)


def execute_model(request: Dict[str, Any]) -> Dict[str, Any]:
    """Execute a model request and return the result."""
    try:
        model = request.get("model")
        system_content, user_content, agent_name, max_output_tokens = _normalize_request(request)

        if not model:
            return {
                "success": False,
                "error": "Missing required field: model",
            }
        if not system_content or not user_content:
            return {
                "success": False,
                "error": "Missing required fields: (system_prompt) or (static_prompt + variable_prompt)",
            }

        provider, model_name = determine_provider(model)
        if provider == "anthropic":
            output = execute_with_anthropic(
                model_name, system_content, user_content, max_output_tokens
            )
        elif provider == "ollama":
            output = execute_with_ollama(model_name, system_content, user_content)
        elif provider == "openai":
            output = execute_with_openai(
                model_name, system_content, user_content, agent_name
            )
        elif provider == "mistral":
            output = execute_with_mistral(model_name, system_content, user_content)
        else:
            return {
                "success": False,
                "error": f"Unknown provider: {provider}",
            }

        return {
            "success": True,
            "output": output,
        }

    except Exception as e:
        return {
            "success": False,
            "error": format_exception(e),
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
