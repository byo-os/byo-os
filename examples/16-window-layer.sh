#!/bin/bash
# Basic window + layer — UI rendered to a texture, composited as a 3D plane
printf '\e_B
  +window main {
    +layer content width=1280 height=720 {
      +view root class="flex flex-col items-center justify-center w-full h-full bg-slate-900" {
        +view card class="flex flex-col items-center gap-4 p-8 bg-slate-800 rounded-2xl border border-slate-700" {
          +text title content="Hello from a Layer!" class="text-3xl text-white"
          +text sub content="This UI is rendered to a texture, then displayed on a 3D plane" class="text-slate-400"
          +view divider class="w-64 h-px bg-slate-600"
          +view badges class="flex flex-row gap-3" {
            +view b1 class="flex items-center justify-center px-3 py-1 rounded-full bg-sky-500/20 border border-sky-500" {
              +text b1t content="Window" class="text-sky-400"
            }
            +view b2 class="flex items-center justify-center px-3 py-1 rounded-full bg-emerald-500/20 border border-emerald-500" {
              +text b2t content="Layer" class="text-emerald-400"
            }
            +view b3 class="flex items-center justify-center px-3 py-1 rounded-full bg-purple-500/20 border border-purple-500" {
              +text b3t content="3D Plane" class="text-purple-400"
            }
          }
        }
      }
    }
  }
\e\\'

sleep 99999
