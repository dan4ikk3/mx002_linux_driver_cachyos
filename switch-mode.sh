#!/bin/bash

# Color codes for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Configuration
PROJECT_DIR="/home/dan4ikk/mx002_linux_driver"
SOURCE_DIR="$PROJECT_DIR/src"
CURRENT_FILE="$SOURCE_DIR/virtual_device.rs"
DRAW_FILE="$SOURCE_DIR/virtual_device.rs.draw"
OSU_FILE="$SOURCE_DIR/virtual_device.rs.osu"
BINARY_PATH="$PROJECT_DIR/target/release/mx002"

# Function to print usage
usage() {
    cat << EOF
${BLUE}=== MX002 Tablet Driver Mode Switcher ===${NC}

Usage: $0 [COMMAND]

Commands:
    ${GREEN}draw${NC}        Switch to drawing mode and build
    ${GREEN}osu${NC}         Switch to osu! mode and build
    ${GREEN}status${NC}      Show current mode
    ${GREEN}help${NC}        Show this help message

Examples:
    $0 draw      # Build for drawing
    $0 osu       # Build for osu!
    $0 status    # Check current mode

EOF
}

# Function to detect current mode
detect_mode() {
    if cmp -s "$CURRENT_FILE" "$DRAW_FILE" 2>/dev/null; then
        echo "draw"
    elif cmp -s "$CURRENT_FILE" "$OSU_FILE" 2>/dev/null; then
        echo "osu"
    else
        echo "unknown"
    fi
}

# Function to show current status
show_status() {
    local mode=$(detect_mode)

    if [ "$mode" = "draw" ]; then
        echo -e "${GREEN}Current Mode: DRAWING${NC}"
    elif [ "$mode" = "osu" ]; then
        echo -e "${GREEN}Current Mode: OSU!${NC}"
    else
        echo -e "${YELLOW}Current Mode: UNKNOWN${NC}"
    fi

    if [ -f "$BINARY_PATH" ]; then
        local binary_size=$(du -h "$BINARY_PATH" | cut -f1)
        echo -e "${BLUE}Binary: ${GREEN}${binary_size}${NC}"
    fi
}

# Function to check if files exist
check_files() {
    if [ ! -f "$DRAW_FILE" ]; then
        echo -e "${RED}Error: Drawing version not found: $DRAW_FILE${NC}"
        exit 1
    fi

    if [ ! -f "$OSU_FILE" ]; then
        echo -e "${RED}Error: Osu! version not found: $OSU_FILE${NC}"
        exit 1
    fi
}

# Function to switch and build for drawing
build_draw() {
    check_files

    echo -e "${YELLOW}Switching to Drawing mode...${NC}"
    cp "$DRAW_FILE" "$CURRENT_FILE"
    echo -e "${GREEN}✓ Drawing mode activated${NC}"

    echo -e "${YELLOW}Building project...${NC}"
    cd "$PROJECT_DIR"
    if cargo build --release 2>&1; then
        echo -e "${GREEN}✓ Build successful${NC}"
        show_status
        return 0
    else
        echo -e "${RED}✗ Build failed${NC}"
        return 1
    fi
}

# Function to switch and build for osu
build_osu() {
    check_files

    echo -e "${YELLOW}Switching to Osu! mode...${NC}"
    cp "$OSU_FILE" "$CURRENT_FILE"
    echo -e "${GREEN}✓ Osu! mode activated${NC}"

    echo -e "${YELLOW}Building project...${NC}"
    cd "$PROJECT_DIR"
    if cargo build --release 2>&1; then
        echo -e "${GREEN}✓ Build successful${NC}"
        show_status
        return 0
    else
        echo -e "${RED}✗ Build failed${NC}"
        return 1
    fi
}

# Main script logic
main() {
    if [ $# -eq 0 ]; then
        usage
        exit 0
    fi

    case "$1" in
        draw)
            build_draw
            ;;
        osu)
            build_osu
            ;;
        status)
            show_status
            ;;
        help)
            usage
            ;;
        *)
            echo -e "${RED}Unknown command: $1${NC}"
            usage
            exit 1
            ;;
    esac
}

main "$@"
