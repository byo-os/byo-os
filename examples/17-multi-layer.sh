#!/bin/bash
# Multiple layers in a window — each rendered to its own texture
printf '\e_B
  +window desktop {
    +layer bg width=1280 height=720 order=0 {
      +view root class="flex flex-col items-center justify-center w-full h-full bg-zinc-950" {
        +text title content="Background Layer" class="text-6xl text-zinc-800"
        +view dots class="flex flex-row flex-wrap gap-4 p-16" {
          +view d1 class="w-4 h-4 rounded-full bg-zinc-900"
          +view d2 class="w-4 h-4 rounded-full bg-zinc-900"
          +view d3 class="w-4 h-4 rounded-full bg-zinc-900"
          +view d4 class="w-4 h-4 rounded-full bg-zinc-900"
          +view d5 class="w-4 h-4 rounded-full bg-zinc-900"
          +view d6 class="w-4 h-4 rounded-full bg-zinc-900"
          +view d7 class="w-4 h-4 rounded-full bg-zinc-900"
          +view d8 class="w-4 h-4 rounded-full bg-zinc-900"
          +view d9 class="w-4 h-4 rounded-full bg-zinc-900"
        }
      }
    }

    +layer panel width=400 height=300 order=1 {
      +view root class="flex flex-col w-full h-full bg-zinc-800 rounded-xl p-6 gap-4 border border-zinc-700" {
        +text title content="Floating Panel" class="text-xl text-white"
        +view divider class="w-full h-px bg-zinc-600"
        +view content class="flex flex-col gap-2" {
          +view row1 class="flex flex-row items-center gap-3" {
            +view dot1 class="w-3 h-3 rounded-full bg-emerald-500"
            +text t1 content="System Online" class="text-zinc-300"
          }
          +view row2 class="flex flex-row items-center gap-3" {
            +view dot2 class="w-3 h-3 rounded-full bg-sky-500"
            +text t2 content="Network Connected" class="text-zinc-300"
          }
          +view row3 class="flex flex-row items-center gap-3" {
            +view dot3 class="w-3 h-3 rounded-full bg-amber-500"
            +text t3 content="Updates Available" class="text-zinc-300"
          }
        }
      }
    }
  }
\e\\'

sleep 99999
