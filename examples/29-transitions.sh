#!/bin/bash
# CSS-like transitions demo — showcases easing curves, spring(), and multi-property transitions
printf '\e_B
  +view root class="flex flex-col items-center justify-center w-full h-full bg-zinc-900 gap-6 p-8" {
    +text title content="Transitions Demo" class="text-2xl text-white"

    +view row1 class="flex gap-6 items-center" {
      +view color-box class="w-32 h-32 rounded-lg bg-blue-500 transition-colors duration-500 ease-in-out"
      +view size-box class="w-32 h-32 rounded-lg bg-emerald-500 transition-all duration-700 ease-out"
      +view shape-box class="w-32 h-32 rounded-lg bg-purple-500 transition-all duration-500 ease-in-out"
    }

    +view row2 class="flex gap-6 items-center" {
      +view spring-box class="w-24 h-24 rounded-lg bg-amber-500" transition="all spring(120,8)"
      +view spring-stiff class="w-24 h-24 rounded-lg bg-rose-500" transition="all spring(200,20)"
      +view spring-soft class="w-24 h-24 rounded-lg bg-cyan-500" transition="all spring(60,6)"
    }

    +text info content="Starting..." class="text-zinc-400"
  }
\e\\'
sleep 2

# Phase 1: Color transitions (ease-in-out)
printf '\e_B
  @view color-box class="w-32 h-32 rounded-lg bg-red-500 transition-colors duration-500 ease-in-out"
  @text info content="Color: blue → red (ease-in-out)"
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
  @text info content="Size grow + rounded (ease-out)"
\e\\'
sleep 1.2

printf '\e_B
  @view size-box class="w-16 h-16 rounded-xl bg-emerald-500 transition-all duration-700 ease-out"
  @view shape-box class="w-48 h-16 rounded-lg bg-indigo-500 transition-all duration-500 ease-in-out"
  @text info content="Size shrink + wide bar"
\e\\'
sleep 1.2

# Phase 3: Spring transitions — bouncy, stiff, and soft
printf '\e_B
  @view spring-box translate-x=100 bg-orange-500
  @view spring-stiff translate-x=100 bg-fuchsia-500
  @view spring-soft translate-x=100 bg-teal-500
  @text info content="Spring: bouncy / stiff / soft →"
\e\\'
sleep 2.5

printf '\e_B
  @view spring-box translate-x="-100" bg-amber-500
  @view spring-stiff translate-x="-100" bg-rose-500
  @view spring-soft translate-x="-100" bg-cyan-500
  @text info content="Spring: ← reverse"
\e\\'
sleep 2.5

# Phase 4: Spring scale + rotate
printf '\e_B
  @view spring-box scale-x=2 scale-y=0.5 rotate=45
  @view spring-stiff rotate=180
  @view spring-soft scale-x=0.5 scale-y=2
  @text info content="Spring: scale + rotate"
\e\\'
sleep 2.5

# Phase 5: Reset all with springs
printf '\e_B
  @view color-box class="w-32 h-32 rounded-full bg-sky-400 transition-all duration-700 ease-in-out"
  @view size-box class="w-32 h-32 rounded-lg bg-emerald-500 transition-all duration-700 ease-out"
  @view shape-box class="w-32 h-32 rounded-lg bg-purple-500 transition-all duration-500 ease-in-out"
  @view spring-box translate-x=0 scale-x=1 scale-y=1 rotate=0
  @view spring-stiff translate-x=0 rotate=0
  @view spring-soft translate-x=0 scale-x=1 scale-y=1
  @text info content="Reset all"
\e\\'
sleep 3

# Phase 6: Continuous spring cycle
while true; do
    # Cycle A: scatter with springs
    printf '\e_B
      @view color-box class="w-48 h-24 rounded-full bg-pink-500 transition-all duration-800 ease-in-out"
      @view size-box class="w-48 h-48 rounded-full bg-teal-400 transition-all duration-600 ease-out"
      @view shape-box class="w-16 h-48 rounded-lg bg-violet-500 transition-all duration-700 ease-in-out"
      @view spring-box translate-x=80 rotate=90 bg-orange-400
      @view spring-stiff translate-x=60 scale-x=1.5 bg-rose-400
      @view spring-soft translate-x="-60" rotate="-45" bg-sky-400
      @text info content="Cycle A"
    \e\\'
    sleep 3

    # Cycle B: gather
    printf '\e_B
      @view color-box class="w-32 h-32 rounded-lg bg-blue-500 transition-all duration-800 ease-in-out"
      @view size-box class="w-32 h-32 rounded-lg bg-emerald-500 transition-all duration-600 ease-out"
      @view shape-box class="w-32 h-32 rounded-lg bg-purple-500 transition-all duration-700 ease-in-out"
      @view spring-box translate-x=0 rotate=0 bg-amber-500
      @view spring-stiff translate-x=0 scale-x=1 bg-rose-500
      @view spring-soft translate-x=0 rotate=0 bg-cyan-500
      @text info content="Cycle B"
    \e\\'
    sleep 3
done
