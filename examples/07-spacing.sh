#!/bin/bash
# Padding & margin showcase
printf '\e_B
  +view root class="flex flex-col p-8 gap-6 w-full h-full bg-zinc-900" {
    +text title content="Padding & Margin" class="text-xl text-white"

    +text l1 content="Padding (p-0, p-2, p-4, p-8, p-16)" class="text-zinc-400"
    +view pads class="flex flex-row gap-4" {
      +view p0 class="bg-sky-900 p-0 rounded" {
        +view p0i class="w-8 h-8 bg-sky-500 rounded"
      }
      +view p2 class="bg-sky-900 p-2 rounded" {
        +view p2i class="w-8 h-8 bg-sky-500 rounded"
      }
      +view p4 class="bg-sky-900 p-4 rounded" {
        +view p4i class="w-8 h-8 bg-sky-500 rounded"
      }
      +view p8 class="bg-sky-900 p-8 rounded" {
        +view p8i class="w-8 h-8 bg-sky-500 rounded"
      }
      +view p16 class="bg-sky-900 p-16 rounded" {
        +view p16i class="w-8 h-8 bg-sky-500 rounded"
      }
    }

    +text l2 content="Directional padding (px-8 py-2)" class="text-zinc-400"
    +view dir class="flex flex-row gap-4" {
      +view dx class="bg-emerald-900 px-8 py-2 rounded" {
        +text dxt content="px-8 py-2" class="text-emerald-200"
      }
      +view dy class="bg-emerald-900 px-2 py-8 rounded" {
        +text dyt content="px-2 py-8" class="text-emerald-200"
      }
    }

    +text l3 content="Margin between items (gap vs margin)" class="text-zinc-400"
    +view mg class="flex flex-row bg-zinc-800 p-4 rounded" {
      +view m1 class="w-16 h-16 bg-rose-500 rounded mr-2"
      +view m2 class="w-16 h-16 bg-rose-500 rounded mx-4"
      +view m3 class="w-16 h-16 bg-rose-500 rounded ml-8"
    }
  }
\e\\'

sleep infinity
