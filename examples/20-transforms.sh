#!/bin/bash
# 2D transforms — translate, rotate, scale on views
printf '\e_B
  +view root class="flex flex-col items-center justify-center w-full h-full bg-zinc-900 gap-8" {
    +text title content="2D View Transforms" class="text-2xl text-white"

    +view row class="flex flex-row gap-16 items-center" {
      +view card1 class="flex items-center justify-center w-32 h-32 rounded-xl bg-sky-500 translate-x-4" {
        +text t1 content="translate-x" class="text-white text-sm"
      }
      +view card2 class="flex items-center justify-center w-32 h-32 rounded-xl bg-emerald-500 rotate-12" {
        +text t2 content="rotate-12" class="text-white text-sm"
      }
      +view card3 class="flex items-center justify-center w-32 h-32 rounded-xl bg-purple-500 scale-75" {
        +text t3 content="scale-75" class="text-white text-sm"
      }
      +view card4 class="flex items-center justify-center w-32 h-32 rounded-xl bg-amber-500 rotate-45 scale-90" {
        +text t4 content="rot+scale" class="text-white text-sm"
      }
    }

    +view row2 class="flex flex-row gap-16 items-center" {
      +view card5 class="flex items-center justify-center w-32 h-32 rounded-xl bg-rose-500 -rotate-12 scale-110" {
        +text t5 content="-rot+scale" class="text-white text-sm"
      }
      +view card6 class="flex items-center justify-center w-32 h-32 rounded-xl bg-teal-500 scale-x-150" {
        +text t6 content="scale-x" class="text-white text-sm"
      }
      +view card7 class="flex items-center justify-center w-32 h-32 rounded-xl bg-indigo-500 translate-y-8" {
        +text t7 content="translate-y" class="text-white text-sm"
      }
      +view card8 class="flex items-center justify-center w-32 h-32 rounded-xl bg-orange-500 -translate-x-4 rotate-[30]" {
        +text t8 content="combined" class="text-white text-sm"
      }
    }
  }
\e\\'

sleep 99999
