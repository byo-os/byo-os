#!/bin/bash
# 3D layer transforms — rotate layers in 3D space
printf '\e_B
  +window scene {
    +layer flat width=400 height=300 order=0
        unlit=true alpha-mode=blend {
      +view flat-root class="flex flex-col items-center justify-center w-full h-full bg-sky-900 rounded-xl" {
        +text flat-t1 content="Flat Layer" class="text-3xl text-white"
        +text flat-t2 content="order=0, no rotation" class="text-sky-300"
      }
    }

    +layer tilted width=400 height=300 order=1
        rotate-y=25 translate-x=100
        unlit=true alpha-mode=blend {
      +view tilted-root class="flex flex-col items-center justify-center w-full h-full bg-emerald-900 rounded-xl" {
        +text tilted-t1 content="Tilted Layer" class="text-3xl text-white"
        +text tilted-t2 content="rotate-y=25" class="text-emerald-300"
      }
    }

    +layer angled width=400 height=300 order=2
        rotate-x=15 rotate-y="-20" translate-x="-100"
        unlit=true alpha-mode=blend {
      +view angled-root class="flex flex-col items-center justify-center w-full h-full bg-purple-900 rounded-xl" {
        +text angled-t1 content="Angled Layer" class="text-3xl text-white"
        +text angled-t2 content="rotate-x=15 rotate-y=-20" class="text-purple-300"
      }
    }
  }
\e\\'

sleep 99999
