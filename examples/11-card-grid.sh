#!/bin/bash
# Card grid — a realistic UI layout
printf '\e_B
  +view root class="flex flex-col p-8 gap-6 w-full h-full bg-zinc-950" {
    +text title content="Project Dashboard" class="text-2xl text-white"
    +text subtitle content="Your recent projects" class="text-zinc-400"

    +view grid class="flex flex-row flex-wrap gap-4" {
      +view card1 class="flex flex-col w-64 bg-zinc-800 rounded-xl p-4 gap-2 border border-zinc-700" {
        +view badge1 class="w-8 h-8 rounded bg-blue-500"
        +text name1 content="Frontend App" class="text-white"
        +text desc1 content="React + TypeScript" class="text-zinc-400"
        +view bar1 class="w-full h-2 rounded-full bg-zinc-700" {
          +view fill1 class="w-3/4 h-2 rounded-full bg-blue-500"
        }
      }

      +view card2 class="flex flex-col w-64 bg-zinc-800 rounded-xl p-4 gap-2 border border-zinc-700" {
        +view badge2 class="w-8 h-8 rounded bg-emerald-500"
        +text name2 content="API Server" class="text-white"
        +text desc2 content="Rust + Axum" class="text-zinc-400"
        +view bar2 class="w-full h-2 rounded-full bg-zinc-700" {
          +view fill2 class="w-full h-2 rounded-full bg-emerald-500"
        }
      }

      +view card3 class="flex flex-col w-64 bg-zinc-800 rounded-xl p-4 gap-2 border border-zinc-700" {
        +view badge3 class="w-8 h-8 rounded bg-amber-500"
        +text name3 content="Mobile App" class="text-white"
        +text desc3 content="Swift + SwiftUI" class="text-zinc-400"
        +view bar3 class="w-full h-2 rounded-full bg-zinc-700" {
          +view fill3 class="w-1/3 h-2 rounded-full bg-amber-500"
        }
      }

      +view card4 class="flex flex-col w-64 bg-zinc-800 rounded-xl p-4 gap-2 border border-zinc-700" {
        +view badge4 class="w-8 h-8 rounded bg-purple-500"
        +text name4 content="ML Pipeline" class="text-white"
        +text desc4 content="Python + PyTorch" class="text-zinc-400"
        +view bar4 class="w-full h-2 rounded-full bg-zinc-700" {
          +view fill4 class="w-1/2 h-2 rounded-full bg-purple-500"
        }
      }
    }
  }
\e\\'

sleep infinity
