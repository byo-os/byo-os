#!/bin/bash
# Borders & rounded corners
printf '\e_B
  +view root class="flex flex-col p-8 gap-6 w-full h-full bg-zinc-900" {
    +text title content="Borders & Rounded Corners" class="text-xl text-white"

    +text l1 content="Border widths" class="text-zinc-400"
    +view bw class="flex flex-row gap-4" {
      +view bw0 class="w-16 h-16 bg-zinc-700 border border-sky-500"
      +view bw2 class="w-16 h-16 bg-zinc-700 border-2 border-sky-500"
      +view bw4 class="w-16 h-16 bg-zinc-700 border-4 border-sky-500"
      +view bw8 class="w-16 h-16 bg-zinc-700 border-8 border-sky-500"
    }

    +text l2 content="Rounded corners" class="text-zinc-400"
    +view rd class="flex flex-row gap-4" {
      +view r0 class="w-16 h-16 bg-pink-500 rounded-none"
      +view r1 class="w-16 h-16 bg-pink-500 rounded-sm"
      +view r2 class="w-16 h-16 bg-pink-500 rounded"
      +view r3 class="w-16 h-16 bg-pink-500 rounded-md"
      +view r4 class="w-16 h-16 bg-pink-500 rounded-lg"
      +view r5 class="w-16 h-16 bg-pink-500 rounded-xl"
      +view r6 class="w-16 h-16 bg-pink-500 rounded-2xl"
      +view r7 class="w-16 h-16 bg-pink-500 rounded-3xl"
      +view r8 class="w-16 h-16 bg-pink-500 rounded-full"
    }

    +text l3 content="Colored borders + rounded" class="text-zinc-400"
    +view combo class="flex flex-row gap-4" {
      +view cb1 class="w-24 h-24 bg-zinc-800 border-2 border-red-500 rounded-lg"
      +view cb2 class="w-24 h-24 bg-zinc-800 border-2 border-green-500 rounded-full"
      +view cb3 class="w-24 h-24 bg-zinc-800 border-4 border-purple-500 rounded-xl"
      +view cb4 class="w-24 h-24 bg-zinc-800 border-4 border-amber-500 rounded"
    }
  }
\e\\'

sleep infinity
