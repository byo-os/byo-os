#!/bin/bash
# Color showcase — Tailwind palette sampler
printf '\e_B
  +view root class="flex flex-col p-8 gap-4 w-full h-full bg-zinc-950" {
    +text title content="Tailwind Color Palette" class="text-xl text-white"

    +view row1 class="flex flex-row gap-2" {
      +view c1 class="w-16 h-16 rounded bg-red-500"
      +view c2 class="w-16 h-16 rounded bg-orange-500"
      +view c3 class="w-16 h-16 rounded bg-amber-500"
      +view c4 class="w-16 h-16 rounded bg-yellow-500"
      +view c5 class="w-16 h-16 rounded bg-lime-500"
      +view c6 class="w-16 h-16 rounded bg-green-500"
      +view c7 class="w-16 h-16 rounded bg-emerald-500"
      +view c8 class="w-16 h-16 rounded bg-teal-500"
      +view c9 class="w-16 h-16 rounded bg-cyan-500"
      +view c10 class="w-16 h-16 rounded bg-sky-500"
      +view c11 class="w-16 h-16 rounded bg-blue-500"
    }
    +view row2 class="flex flex-row gap-2" {
      +view c12 class="w-16 h-16 rounded bg-indigo-500"
      +view c13 class="w-16 h-16 rounded bg-violet-500"
      +view c14 class="w-16 h-16 rounded bg-purple-500"
      +view c15 class="w-16 h-16 rounded bg-fuchsia-500"
      +view c16 class="w-16 h-16 rounded bg-pink-500"
      +view c17 class="w-16 h-16 rounded bg-rose-500"
      +view c18 class="w-16 h-16 rounded bg-slate-500"
      +view c19 class="w-16 h-16 rounded bg-gray-500"
      +view c20 class="w-16 h-16 rounded bg-zinc-500"
      +view c21 class="w-16 h-16 rounded bg-neutral-500"
      +view c22 class="w-16 h-16 rounded bg-stone-500"
    }

    +text shade-title content="Blue shades" class="text-white"
    +view row3 class="flex flex-row gap-2" {
      +view b1 class="w-16 h-16 rounded bg-blue-50"
      +view b2 class="w-16 h-16 rounded bg-blue-100"
      +view b3 class="w-16 h-16 rounded bg-blue-200"
      +view b4 class="w-16 h-16 rounded bg-blue-300"
      +view b5 class="w-16 h-16 rounded bg-blue-400"
      +view b6 class="w-16 h-16 rounded bg-blue-500"
      +view b7 class="w-16 h-16 rounded bg-blue-600"
      +view b8 class="w-16 h-16 rounded bg-blue-700"
      +view b9 class="w-16 h-16 rounded bg-blue-800"
      +view b10 class="w-16 h-16 rounded bg-blue-900"
      +view b11 class="w-16 h-16 rounded bg-blue-950"
    }
  }
\e\\'

sleep 99999
