#!/bin/bash
# CSS-like transitions demo — showcases color, size, opacity, transform, and multi-property transitions
printf '\e_B
  +view root class="flex flex-col items-center justify-center w-full h-full bg-zinc-900 gap-6 p-8" {
    +text title content="Transitions Demo" class="text-2xl text-white"

    +view row1 class="flex gap-6 items-center" {
      +view color-box class="w-32 h-32 rounded-lg bg-blue-500 transition-colors duration-500 ease-in-out"
      +view size-box class="w-32 h-32 rounded-lg bg-emerald-500 transition-all duration-700 ease-out"
      +view shape-box class="w-32 h-32 rounded-lg bg-purple-500 transition-all duration-500 ease-in-out"
    }

    +view row2 class="flex gap-6 items-center" {
      +view move-box class="w-24 h-24 rounded-lg bg-amber-500 transition-all duration-600 ease-in-out"
      +view spin-box class="w-24 h-24 rounded-lg bg-rose-500 transition-all duration-800 ease-in-out"
      +view scale-box class="w-24 h-24 rounded-lg bg-cyan-500 transition-all duration-500 ease-out"
    }

    +text info content="Starting..." class="text-zinc-400"
  }
\e\\'
sleep 2

# Phase 1: Color transitions
printf '\e_B
  @view color-box class="w-32 h-32 rounded-lg bg-red-500 transition-colors duration-500 ease-in-out"
  @text info content="Color: blue → red"
\e\\'
sleep 1

printf '\e_B
  @view color-box class="w-32 h-32 rounded-lg bg-amber-400 transition-colors duration-500 ease-in-out"
  @text info content="Color: red → amber"
\e\\'
sleep 1

printf '\e_B
  @view color-box class="w-32 h-32 rounded-lg bg-emerald-400 transition-colors duration-500 ease-in-out"
  @text info content="Color: amber → emerald"
\e\\'
sleep 1

# Phase 2: Size + shape transitions
printf '\e_B
  @view size-box class="w-48 h-48 rounded-lg bg-emerald-500 transition-all duration-700 ease-out"
  @view shape-box class="w-32 h-32 rounded-full bg-purple-500 transition-all duration-500 ease-in-out"
  @text info content="Size grow + rounded"
\e\\'
sleep 1.2

printf '\e_B
  @view size-box class="w-16 h-16 rounded-xl bg-emerald-500 transition-all duration-700 ease-out"
  @view shape-box class="w-48 h-16 rounded-lg bg-indigo-500 transition-all duration-500 ease-in-out"
  @text info content="Size shrink + wide bar"
\e\\'
sleep 1.2

# Phase 3: Transform transitions
printf '\e_B
  @view move-box class="w-24 h-24 rounded-lg bg-amber-500 transition-all duration-600 ease-in-out" translate-x=80
  @view spin-box class="w-24 h-24 rounded-lg bg-rose-500 transition-all duration-800 ease-in-out" rotate=180
  @view scale-box class="w-24 h-24 rounded-lg bg-cyan-500 transition-all duration-500 ease-out" scale-x=1.5 scale-y=0.5
  @text info content="Translate + Rotate + Scale"
\e\\'
sleep 1.5

printf '\e_B
  @view move-box class="w-24 h-24 rounded-lg bg-amber-500 transition-all duration-600 ease-in-out" translate-x="-80"
  @view spin-box class="w-24 h-24 rounded-lg bg-rose-500 transition-all duration-800 ease-in-out" rotate=0
  @view scale-box class="w-24 h-24 rounded-lg bg-cyan-500 transition-all duration-500 ease-out" scale-x=0.5 scale-y=1.5
  @text info content="Reverse transforms"
\e\\'
sleep 1.5

# Phase 4: Multi-property simultaneous
printf '\e_B
  @view color-box class="w-32 h-32 rounded-full bg-sky-400 transition-all duration-700 ease-in-out"
  @view size-box class="w-32 h-32 rounded-lg bg-emerald-500 transition-all duration-700 ease-out"
  @view shape-box class="w-32 h-32 rounded-lg bg-purple-500 transition-all duration-500 ease-in-out"
  @view move-box class="w-24 h-24 rounded-lg bg-amber-500 transition-all duration-600 ease-in-out" translate-x=0
  @view spin-box class="w-24 h-24 rounded-full bg-fuchsia-500 transition-all duration-800 ease-in-out" rotate=360
  @view scale-box class="w-24 h-24 rounded-lg bg-cyan-500 transition-all duration-500 ease-out" scale-x=1 scale-y=1
  @text info content="Reset all + multi-property"
\e\\'
sleep 2

# Phase 5: Continuous cycle
while true; do
    # Cycle A: scatter
    printf '\e_B
      @view color-box class="w-48 h-24 rounded-full bg-pink-500 transition-all duration-800 ease-in-out"
      @view size-box class="w-48 h-48 rounded-full bg-teal-400 transition-all duration-600 ease-out"
      @view shape-box class="w-16 h-48 rounded-lg bg-violet-500 transition-all duration-700 ease-in-out"
      @view move-box class="w-24 h-24 rounded-full bg-orange-400 transition-all duration-500 ease-in-out" translate-x=60
      @view spin-box class="w-32 h-32 rounded-lg bg-rose-400 transition-all duration-800 ease-in-out" rotate=90
      @view scale-box class="w-24 h-24 rounded-lg bg-sky-400 transition-all duration-500 ease-out" scale-x=2 scale-y=0.5
      @text info content="Cycle A"
    \e\\'
    sleep 2

    # Cycle B: gather
    printf '\e_B
      @view color-box class="w-32 h-32 rounded-lg bg-blue-500 transition-all duration-800 ease-in-out"
      @view size-box class="w-32 h-32 rounded-lg bg-emerald-500 transition-all duration-600 ease-out"
      @view shape-box class="w-32 h-32 rounded-lg bg-purple-500 transition-all duration-700 ease-in-out"
      @view move-box class="w-24 h-24 rounded-lg bg-amber-500 transition-all duration-500 ease-in-out" translate-x=0
      @view spin-box class="w-24 h-24 rounded-lg bg-rose-500 transition-all duration-800 ease-in-out" rotate=0
      @view scale-box class="w-24 h-24 rounded-lg bg-cyan-500 transition-all duration-500 ease-out" scale-x=1 scale-y=1
      @text info content="Cycle B"
    \e\\'
    sleep 2
done
