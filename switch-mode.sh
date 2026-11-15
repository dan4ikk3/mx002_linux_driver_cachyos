#!/bin/bash
# Сборка и запуск драйвера в нужном режиме.
# Режим выбирается аргументом --mode, подменять исходники больше не нужно.

RED='\033[0;31m'; GREEN='\033[0;32m'; YELLOW='\033[1;33m'; BLUE='\033[0;34m'; NC='\033[0m'

PROJECT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BINARY_PATH="$PROJECT_DIR/target/release/mx002"

usage() {
    echo -e "${BLUE}=== MX002 Tablet Driver ===${NC}"
    echo
    echo "Usage: $0 COMMAND [driver options...]"
    echo
    echo "Commands:"
    echo -e "    ${GREEN}draw${NC}        Собрать и запустить в режиме рисования"
    echo -e "    ${GREEN}osu${NC}         Собрать и запустить в режиме osu!"
    echo -e "    ${GREEN}calibrate${NC}   Показать диапазон давления пера (подбор --base/--max)"
    echo -e "    ${GREEN}build${NC}       Только собрать"
    echo -e "    ${GREEN}help${NC}        Эта справка"
    echo
    echo "Дальнейшие аргументы передаются драйверу, например:"
    echo "    $0 draw --max 650 --curve 0.8"
    echo "    sudo $0 draw        # если нет udev-правила"
}

build() {
    echo -e "${YELLOW}Building...${NC}"
    (cd "$PROJECT_DIR" && cargo build --release) || { echo -e "${RED}✗ Build failed${NC}"; exit 1; }
    echo -e "${GREEN}✓ Build successful${NC}"
}

cmd="${1:-help}"
shift || true
case "$cmd" in
    draw|osu)  build; exec "$BINARY_PATH" --mode "$cmd" "$@" ;;
    calibrate) build; exec "$BINARY_PATH" --calibrate "$@" ;;
    build)     build ;;
    help|-h|--help) usage ;;
    *) echo -e "${RED}Unknown command: $cmd${NC}"; usage; exit 1 ;;
esac
