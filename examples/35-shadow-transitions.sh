#!/bin/bash
# Animated shadow transitions — hover-like effects, shadow morphing, and list padding
printf '\e_B
  +view root class="flex flex-col items-center justify-center w-full h-full bg-zinc-200 gap-5 p-6" {
    +text title content="Shadow Transitions" class="text-2xl text-zinc-800"

    +view row1 class="flex gap-5 items-center p-3" {
      +view card1 class="flex items-center justify-center w-32 h-20 rounded-xl bg-white shadow-sm transition duration-500" {
        +text c1t content="Lift" class="text-sm text-zinc-600"
      }
      +view card2 class="flex items-center justify-center w-32 h-20 rounded-xl bg-white shadow-sm transition duration-500" {
        +text c2t content="Glow" class="text-sm text-zinc-600"
      }
      +view card3 class="flex items-center justify-center w-32 h-20 rounded-xl bg-white shadow-sm transition duration-500" {
        +text c3t content="Vanish" class="text-sm text-zinc-600"
      }
    }

    +view row2 class="flex gap-5 items-center p-3" {
      +view orb1 class="w-14 h-14 rounded-full bg-blue-500 shadow-lg shadow-blue-500 transition duration-700"
      +view orb2 class="w-14 h-14 rounded-full bg-emerald-500 shadow-lg shadow-emerald-500 transition duration-700"
      +view orb3 class="w-14 h-14 rounded-full bg-rose-500 shadow-lg shadow-rose-500 transition duration-700"
    }

    +view row3 class="flex gap-5 items-center p-3" {
      +view multi1 class="flex items-center justify-center w-32 h-20 rounded-xl bg-white transition duration-700" box-shadow="0 6px 0 #3b82f6" {
        +text m1t content="1 to 3" class="text-sm text-zinc-600"
      }
      +view multi2 class="flex items-center justify-center w-32 h-20 rounded-xl bg-white transition duration-700" box-shadow="-6px 6px 0 #ef4444, 0 6px 0 #3b82f6, 6px 6px 0 #22c55e" {
        +text m2t content="3 to 1" class="text-sm text-zinc-600"
      }
      +view multi3 class="flex items-center justify-center w-32 h-20 rounded-xl bg-white transition duration-700" box-shadow="-6px 6px 0 #ef4444, 6px 6px 0 #22c55e" {
        +text m3t content="morph" class="text-sm text-zinc-600"
      }
    }

    +text info content="Starting..." class="text-zinc-400"
  }
\e\\'
sleep 2

# Phase 1: Lift cards (sm -> 2xl)
printf '\e_B
  @view card1 class="flex items-center justify-center w-32 h-20 rounded-xl bg-white shadow-2xl transition duration-500"
  @text info content="Cards: shadow-sm to shadow-2xl"
\e\\'
sleep 0.8

printf '\e_B
  @view card2 class="flex items-center justify-center w-32 h-20 rounded-xl bg-white shadow-2xl transition duration-500"
\e\\'
sleep 0.8

printf '\e_B
  @view card3 class="flex items-center justify-center w-32 h-20 rounded-xl bg-white shadow-2xl transition duration-500"
\e\\'
sleep 1.5

# Phase 2: Color glow on card2
printf '\e_B
  @view card2 class="flex items-center justify-center w-32 h-20 rounded-xl bg-white transition duration-700" box-shadow="0 0 24px rgba(59,130,246,0.6), 0 0 48px rgba(59,130,246,0.3)"
  @text info content="Blue glow via CSS box-shadow"
\e\\'
sleep 2

# Phase 3: Vanish card3 shadow
printf '\e_B
  @view card3 class="flex items-center justify-center w-32 h-20 rounded-xl bg-white shadow-none transition duration-500"
  @text info content="shadow-none: fade out"
\e\\'
sleep 1.5

# Phase 4: Orbs — color swap
printf '\e_B
  @view orb1 class="w-14 h-14 rounded-full bg-rose-500 shadow-lg shadow-rose-500 transition duration-700"
  @view orb2 class="w-14 h-14 rounded-full bg-blue-500 shadow-lg shadow-blue-500 transition duration-700"
  @view orb3 class="w-14 h-14 rounded-full bg-emerald-500 shadow-lg shadow-emerald-500 transition duration-700"
  @text info content="Orbs: color rotation"
\e\\'
sleep 2

# Phase 5: Multi-shadow list transitions
# 1→3: one center shadow splits into three directional shadows
printf '\e_B
  @view multi1 box-shadow="-6px 6px 0 #ef4444, 0 6px 0 #3b82f6, 6px 6px 0 #22c55e"
  @text info content="1 to 3: new shadows fade in from transparent"
\e\\'
sleep 2

# 3→1: three directional shadows collapse into one center shadow
printf '\e_B
  @view multi2 box-shadow="0 6px 0 #a855f7"
  @text info content="3 to 1: extras fade out to transparent"
\e\\'
sleep 2

# same count, different values: two shadows swap sides and colors
printf '\e_B
  @view multi3 box-shadow="6px 6px 0 #ef4444, -6px 6px 0 #22c55e"
  @text info content="Same count: shadows swap sides and colors"
\e\\'
sleep 2

# Cycle
while true; do
  # A: settle
  printf '\e_B
    @view card1 class="flex items-center justify-center w-32 h-20 rounded-xl bg-white shadow-sm transition duration-500"
    @view card2 class="flex items-center justify-center w-32 h-20 rounded-xl bg-white shadow-sm transition duration-700"
    @view card3 class="flex items-center justify-center w-32 h-20 rounded-xl bg-white shadow-sm transition duration-500"
    @view orb1 class="w-14 h-14 rounded-full bg-blue-500 shadow-lg shadow-blue-500 transition duration-700"
    @view orb2 class="w-14 h-14 rounded-full bg-emerald-500 shadow-lg shadow-emerald-500 transition duration-700"
    @view orb3 class="w-14 h-14 rounded-full bg-rose-500 shadow-lg shadow-rose-500 transition duration-700"
    @view multi1 box-shadow="0 6px 0 #3b82f6"
    @view multi2 box-shadow="-6px 6px 0 #ef4444, 0 6px 0 #3b82f6, 6px 6px 0 #22c55e"
    @view multi3 box-shadow="-6px 6px 0 #ef4444, 6px 6px 0 #22c55e"
    @text info content="Settle"
  \e\\'
  sleep 3

  # B: lift + glow + multi transitions
  printf '\e_B
    @view card1 class="flex items-center justify-center w-32 h-20 rounded-xl bg-white shadow-2xl transition duration-500"
    @view card2 class="flex items-center justify-center w-32 h-20 rounded-xl bg-white transition duration-700" box-shadow="0 0 24px rgba(59,130,246,0.6), 0 0 48px rgba(59,130,246,0.3)"
    @view card3 class="flex items-center justify-center w-32 h-20 rounded-xl bg-white shadow-none transition duration-500"
    @view orb1 class="w-14 h-14 rounded-full bg-rose-500 shadow-2xl shadow-rose-500 transition duration-700"
    @view orb2 class="w-14 h-14 rounded-full bg-blue-500 shadow-2xl shadow-blue-500 transition duration-700"
    @view orb3 class="w-14 h-14 rounded-full bg-emerald-500 shadow-2xl shadow-emerald-500 transition duration-700"
    @view multi1 box-shadow="-6px 6px 0 #ef4444, 0 6px 0 #3b82f6, 6px 6px 0 #22c55e"
    @view multi2 box-shadow="0 6px 0 #a855f7"
    @view multi3 box-shadow="6px 6px 0 #ef4444, -6px 6px 0 #22c55e"
    @text info content="Lift / Glow / Multi-shadow"
  \e\\'
  sleep 3
done
