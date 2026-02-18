#!/bin/bash
# Create & destroy — items appear and disappear
printf '\e_B
  +view root class="flex flex-col p-8 gap-4 w-full h-full bg-zinc-900" {
    +text title content="Create & Destroy" class="text-xl text-white"
    +text status content="Watch items appear and disappear..." class="text-zinc-500"
    +view container class="flex flex-row flex-wrap gap-2"
  }
\e\\'
sleep 1

COLORS=("red" "orange" "amber" "yellow" "lime" "green" "emerald" "teal" "cyan" "sky" "blue" "indigo" "violet" "purple" "fuchsia" "pink" "rose")

# Phase 1: Add items one by one
for i in $(seq 1 16); do
    COLOR=${COLORS[$((i - 1))]}
    printf '\e_B
      @view container {
        +view item%d class="w-12 h-12 rounded bg-%s-500"
      }
    \e\\' "$i" "$COLOR"
    sleep 0.3
done

printf '\e_B @text status content="All items created! Removing in 2s..." \e\\'
sleep 2

# Phase 2: Remove items one by one (reverse order)
for i in $(seq 16 -1 1); do
    printf '\e_B -view item%d \e\\' "$i"
    sleep 0.2
done

printf '\e_B @text status content="All gone! Re-creating in 2s..." \e\\'
sleep 2

# Phase 3: Add them all at once
CMD='\e_B @view container {'
for i in $(seq 1 16); do
    COLOR=${COLORS[$((i - 1))]}
    CMD="$CMD +view item$i class=\"w-12 h-12 rounded-full bg-${COLOR}-500\""
done
CMD="$CMD } \e\\"
printf "$CMD"

printf '\e_B @text status content="All back! (rounded-full this time)" \e\\'
sleep infinity
