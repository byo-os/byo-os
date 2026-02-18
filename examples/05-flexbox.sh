#!/bin/bash
# Flexbox layouts — row, column, wrap, alignment, justify
printf '\e_B
  +view root class="flex flex-col p-8 gap-6 w-full h-full bg-zinc-900" {
    +text title content="Flexbox Layouts" class="text-xl text-white"

    +text l1 content="flex-row + justify-between" class="text-zinc-400"
    +view row1 class="flex flex-row justify-between bg-zinc-800 p-4 rounded" {
      +view a class="w-16 h-16 bg-red-500 rounded"
      +view b class="w-16 h-16 bg-green-500 rounded"
      +view c class="w-16 h-16 bg-blue-500 rounded"
    }

    +text l2 content="flex-row + items-center + gap-4" class="text-zinc-400"
    +view row2 class="flex flex-row items-center gap-4 bg-zinc-800 p-4 rounded" {
      +view d class="w-8 h-8 bg-amber-500 rounded"
      +view e class="w-8 h-16 bg-amber-500 rounded"
      +view f class="w-8 h-24 bg-amber-500 rounded"
      +view g class="w-8 h-16 bg-amber-500 rounded"
      +view h class="w-8 h-8 bg-amber-500 rounded"
    }

    +text l3 content="flex-col + items-center" class="text-zinc-400"
    +view col1 class="flex flex-col items-center gap-2 bg-zinc-800 p-4 rounded" {
      +view i class="w-32 h-8 bg-purple-500 rounded"
      +view j class="w-24 h-8 bg-purple-500 rounded"
      +view k class="w-16 h-8 bg-purple-500 rounded"
    }

    +text l4 content="flex-wrap" class="text-zinc-400"
    +view wrap class="flex flex-row flex-wrap gap-2 bg-zinc-800 p-4 rounded" {
      +view w1 class="w-32 h-12 bg-teal-500 rounded"
      +view w2 class="w-32 h-12 bg-teal-500 rounded"
      +view w3 class="w-32 h-12 bg-teal-500 rounded"
      +view w4 class="w-32 h-12 bg-teal-500 rounded"
      +view w5 class="w-32 h-12 bg-teal-500 rounded"
      +view w6 class="w-32 h-12 bg-teal-500 rounded"
      +view w7 class="w-32 h-12 bg-teal-500 rounded"
      +view w8 class="w-32 h-12 bg-teal-500 rounded"
    }
  }
\e\\'

sleep 99999
