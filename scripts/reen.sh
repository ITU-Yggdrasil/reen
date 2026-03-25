#!/usr/bin/env bash
# Source in bash/zsh: source scripts/reen.sh

reen() {
  local env_file_args=()
  local env_var_args=()
  if [[ -f ".env" ]]; then
    env_file_args=(--env-file "$(pwd)/.env")
  fi
  if [[ -n "${MISTRAL_API_KEY:-}" ]]; then
    env_var_args=(-e "MISTRAL_API_KEY=${MISTRAL_API_KEY}")
  fi

  if [[ -z "$(docker image ls -q reen:latest)" ]]; then
    echo "Error: Docker image 'reen:latest' was not found locally."
    echo "Build it with: docker build -t reen:latest ."
    return 1
  fi

  docker run --rm -it \
    "${env_file_args[@]}" \
    "${env_var_args[@]}" \
    -v "$(pwd):/work" \
    -w /work \
    reen:latest "$@"
}
