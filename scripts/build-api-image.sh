#!/usr/bin/env bash
set -euo pipefail

IMAGE="kate522/aksrtblog-api"
TAG=""
PLATFORM="linux/amd64"
PUSH=0
USE_CACHE=0
DRY_RUN=0

while [ "$#" -gt 0 ]; do
  case "$1" in
    --image)
      IMAGE="$2"
      shift 2
      ;;
    --tag)
      TAG="$2"
      shift 2
      ;;
    --platform)
      PLATFORM="$2"
      shift 2
      ;;
    --push)
      PUSH=1
      shift
      ;;
    --use-cache)
      USE_CACHE=1
      shift
      ;;
    --dry-run)
      DRY_RUN=1
      shift
      ;;
    *)
      echo "Unknown argument: $1" >&2
      exit 2
      ;;
  esac
done

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BACKEND_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"

if [ -z "$TAG" ]; then
  TIMESTAMP="$(date +%Y%m%d-%H%M%S)"
  GIT_SHA="$(git -C "$BACKEND_DIR" rev-parse --short HEAD 2>/dev/null || echo nogit)"
  TAG="${TIMESTAMP}-${GIT_SHA}"
fi

BUILD_ARGS=(
  buildx build
  --platform "$PLATFORM"
  --pull
  -t "${IMAGE}:${TAG}"
  -t "${IMAGE}:latest"
)

if [ "$USE_CACHE" -eq 0 ]; then
  BUILD_ARGS+=(--no-cache)
fi

if [ "$PUSH" -eq 1 ]; then
  BUILD_ARGS+=(--push)
else
  BUILD_ARGS+=(--load)
fi

BUILD_ARGS+=("$BACKEND_DIR")

echo "Building API image:"
echo "  ${IMAGE}:${TAG}"
echo "  ${IMAGE}:latest"
echo
echo "Command:"
printf 'docker'
printf ' %q' "${BUILD_ARGS[@]}"
printf '\n'

if [ "$DRY_RUN" -eq 1 ]; then
  exit 0
fi

docker "${BUILD_ARGS[@]}"

