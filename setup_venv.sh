#!/bin/bash
# Setup script to create/use Python virtual environment and install requirements

set -e  # Exit on error

# Colors for output
GREEN='\033[0;32m'
RED='\033[0;31m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# Find project root (directory containing this script)
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$SCRIPT_DIR"
VENV_DIR="$ROOT_DIR/.venv"

echo -e "${YELLOW}Setting up Python virtual environment...${NC}"
echo ""

# Check if Python 3 is available
if ! command -v python3 &> /dev/null; then
    echo -e "${RED}Error: python3 not found. Please install Python 3.${NC}"
    exit 1
fi

PYTHON_VERSION=$(python3 --version)
echo -e "${GREEN}✓${NC} Found $PYTHON_VERSION"

# Create venv if it doesn't exist
if [ ! -d "$VENV_DIR" ]; then
    echo -e "${YELLOW}Creating virtual environment at $VENV_DIR...${NC}"
    python3 -m venv "$VENV_DIR"
    echo -e "${GREEN}✓${NC} Virtual environment created"
else
    echo -e "${GREEN}✓${NC} Virtual environment already exists at $VENV_DIR"
fi

# Activate venv and upgrade pip
echo -e "${YELLOW}Upgrading pip...${NC}"
source "$VENV_DIR/bin/activate"
pip install --upgrade pip --quiet
echo -e "${GREEN}✓${NC} pip upgraded"

# Install requirements if requirements.txt exists
if [ -f "$ROOT_DIR/requirements.txt" ]; then
    echo -e "${YELLOW}Installing requirements from requirements.txt...${NC}"
    pip install -r "$ROOT_DIR/requirements.txt" --quiet
    echo -e "${GREEN}✓${NC} Requirements installed"
else
    echo -e "${YELLOW}⚠${NC} requirements.txt not found, skipping installation"
fi

echo ""
echo -e "${GREEN}=====================================${NC}"
echo -e "${GREEN}✓ Virtual environment setup complete!${NC}"
echo -e "${GREEN}=====================================${NC}"
echo ""
echo "To activate the virtual environment manually, run:"
echo "  source $VENV_DIR/bin/activate"
echo ""
echo "The runner.py script will automatically use this venv if it exists."

