#!/bin/bash
# Section sizes of the built app. `.data` must be empty on this target, and
# `.bss` must fit SRAM (36K on Flex) alongside the stack.
source "$(dirname "$0")/../env.sh"
arm-none-eabi-readelf -S "$APP_ELF" 2>/dev/null | grep -E "Name|\.text|\.data|\.bss|\.rodata|\.nvm" \
  || readelf -S "$APP_ELF" | grep -E "Name|\.text|\.data|\.bss|\.rodata|\.nvm"
