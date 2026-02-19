#!/bin/bash
# Example 32: Terminal emulator — default root tty
#
# Writes ANSI-colored output directly to the default root terminal.
# No UI framing — just passthrough to the implicit "/" tty.

printf '\e[1;36m=== BYO/OS Terminal Emulator ===\e[0m\n\n'

printf '\e[31mRed\e[0m '
printf '\e[32mGreen\e[0m '
printf '\e[33mYellow\e[0m '
printf '\e[34mBlue\e[0m '
printf '\e[35mMagenta\e[0m '
printf '\e[36mCyan\e[0m\n\n'

printf '\e[1mBold text\e[0m and \e[4munderlined text\e[0m\n\n'

# 256-color palette
for i in $(seq 0 15); do
    printf '\e[48;5;%dm  ' "$i"
done
printf '\e[0m\n'

for i in $(seq 16 231); do
    printf '\e[48;5;%dm ' "$i"
    if (( (i - 15) % 36 == 0 )); then
        printf '\e[0m\n'
    fi
done
printf '\e[0m\n'

printf '\nHello from the terminal emulator!\n'
printf '\e[36m%s\e[0m\n' "$(date)"

# Keep running so the window stays open
sleep 99999
