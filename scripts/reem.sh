#!/usr/bin/env bash
# Source in bash/zsh: source scripts/reem.sh

reem() {
  if ! docker image inspect reen:latest >/dev/null 2>&1; then
    echo "Error: Docker image 'reen:latest' was not found locally."
    echo "Build it with: docker build -t reen:latest ."
    return 1
  fi

  docker run --rm -it \
    -e "MISTRAL_API_KEY=${MISTRAL_API_KEY}" \
    -v "$(pwd):/work" \
    -w /work \
    reen:latest "$@"
}
