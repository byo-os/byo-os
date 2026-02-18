#!/bin/bash
# Sizing — various width/height classes
printf '\e_B
  +view root class="flex flex-col p-8 gap-4 w-full h-full bg-zinc-900" {
    +text title content="Sizing Demo" class="text-xl text-white"

    +text l1 content="Fixed widths (w-8, w-16, w-32, w-64, w-96)" class="text-zinc-400"
    +view sizes class="flex flex-col gap-2" {
      +view s1 class="w-8 h-4 bg-sky-500 rounded"
      +view s2 class="w-16 h-4 bg-sky-500 rounded"
      +view s3 class="w-32 h-4 bg-sky-500 rounded"
      +view s4 class="w-64 h-4 bg-sky-500 rounded"
      +view s5 class="w-96 h-4 bg-sky-500 rounded"
    }

    +text l2 content="Fractional widths (1/4, 1/3, 1/2, 2/3, 3/4, full)" class="text-zinc-400"
    +view fracs class="flex flex-col gap-2 w-full" {
      +view f1 class="w-1/4 h-4 bg-emerald-500 rounded"
      +view f2 class="w-1/3 h-4 bg-emerald-500 rounded"
      +view f3 class="w-1/2 h-4 bg-emerald-500 rounded"
      +view f4 class="w-2/3 h-4 bg-emerald-500 rounded"
      +view f5 class="w-3/4 h-4 bg-emerald-500 rounded"
      +view f6 class="w-full h-4 bg-emerald-500 rounded"
    }

    +text l3 content="Heights (h-8, h-16, h-24, h-32)" class="text-zinc-400"
    +view heights class="flex flex-row gap-2 items-end" {
      +view h1 class="w-16 h-8 bg-violet-500 rounded"
      +view h2 class="w-16 h-16 bg-violet-500 rounded"
      +view h3 class="w-16 h-24 bg-violet-500 rounded"
      +view h4 class="w-16 h-32 bg-violet-500 rounded"
    }
  }
\e\\'

sleep infinity
