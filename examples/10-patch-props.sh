#!/bin/bash
# Patch props — animate color and size changes
printf '\e_B
  +view root class="flex flex-col items-center justify-center w-full h-full bg-zinc-900 gap-8" {
    +text title content="Patch Props Demo" class="text-xl text-white"
    +view box class="w-32 h-32 rounded-lg bg-blue-500"
    +text info content="Cycling colors..." class="text-zinc-400"
  }
\e\\'
sleep 1

CLASSES=(
  "w-32 h-32 rounded-lg bg-blue-500"
  "w-48 h-32 rounded-lg bg-red-500"
  "w-48 h-48 rounded-xl bg-green-500"
  "w-64 h-48 rounded-2xl bg-purple-500"
  "w-64 h-64 rounded-full bg-amber-500"
  "w-48 h-48 rounded-full bg-pink-500"
  "w-32 h-32 rounded-full bg-cyan-500"
  "w-16 h-16 rounded bg-emerald-500"
  "w-32 h-32 rounded-lg bg-blue-500"
)

LABELS=(
  "Blue square (32x32)"
  "Red wide (48x32)"
  "Green bigger (48x48)"
  "Purple wide (64x48)"
  "Amber circle (64x64)"
  "Pink shrinking circle"
  "Cyan medium circle"
  "Emerald tiny"
  "Back to start!"
)

while true; do
    for i in "${!CLASSES[@]}"; do
        printf '\e_B
          @view box class="%s"
          @text info content="%s"
        \e\\' "${CLASSES[$i]}" "${LABELS[$i]}"
        sleep 1
    done
done
