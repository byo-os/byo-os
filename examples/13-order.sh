#!/bin/bash
# Order prop — reordering children dynamically
printf '\e_B
  +view root class="flex flex-col items-center justify-center w-full h-full bg-zinc-900 gap-8" {
    +text title content="Order Prop Demo" class="text-xl text-white"
    +text info content="Items will shuffle every 2 seconds" class="text-zinc-500"

    +view container class="flex flex-row gap-4" {
      +view a class="flex items-center justify-center w-16 h-16 rounded-lg bg-red-500" order=1 {
        +text la content="1" class="text-white text-2xl"
      }
      +view b class="flex items-center justify-center w-16 h-16 rounded-lg bg-green-500" order=2 {
        +text lb content="2" class="text-white text-2xl"
      }
      +view c class="flex items-center justify-center w-16 h-16 rounded-lg bg-blue-500" order=3 {
        +text lc content="3" class="text-white text-2xl"
      }
      +view d class="flex items-center justify-center w-16 h-16 rounded-lg bg-amber-500" order=4 {
        +text ld content="4" class="text-white text-2xl"
      }
      +view e class="flex items-center justify-center w-16 h-16 rounded-lg bg-purple-500" order=5 {
        +text le content="5" class="text-white text-2xl"
      }
    }
  }
\e\\'
sleep 2

# Shuffle orders
STEP=0
while true; do
    case $((STEP % 4)) in
        0) O1=5; O2=4; O3=3; O4=2; O5=1; MSG="Reversed!" ;;
        1) O1=3; O2=1; O3=5; O4=2; O5=4; MSG="Shuffled!" ;;
        2) O1=2; O2=4; O3=1; O4=5; O5=3; MSG="Scrambled!" ;;
        3) O1=1; O2=2; O3=3; O4=4; O5=5; MSG="Back to normal!" ;;
    esac
    printf '\e_B
      @view a order=%d
      @view b order=%d
      @view c order=%d
      @view d order=%d
      @view e order=%d
      @text info content="%s"
    \e\\' "$O1" "$O2" "$O3" "$O4" "$O5" "$MSG"
    STEP=$((STEP + 1))
    sleep 2
done
