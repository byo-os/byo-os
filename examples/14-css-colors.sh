#!/bin/bash
# CSS color formats — hex, rgb, hsl, named colors
printf '\e_B
  +view root class="flex flex-col p-8 gap-4 w-full h-full bg-zinc-900" {
    +text title content="CSS Color Formats" class="text-xl text-white"

    +text l1 content="Hex colors" class="text-zinc-400"
    +view hex class="flex flex-row gap-2" {
      +view h1 class="w-16 h-16 rounded" background-color="#ff0000"
      +view h2 class="w-16 h-16 rounded" background-color="#00ff00"
      +view h3 class="w-16 h-16 rounded" background-color="#0000ff"
      +view h4 class="w-16 h-16 rounded" background-color="#ff6600"
      +view h5 class="w-16 h-16 rounded" background-color="#663399"
    }

    +text l2 content="RGB colors" class="text-zinc-400"
    +view rgb class="flex flex-row gap-2" {
      +view r1 class="w-16 h-16 rounded" background-color="rgb(255, 99, 71)"
      +view r2 class="w-16 h-16 rounded" background-color="rgb(64, 224, 208)"
      +view r3 class="w-16 h-16 rounded" background-color="rgb(255, 215, 0)"
      +view r4 class="w-16 h-16 rounded" background-color="rgb(148, 103, 189)"
      +view r5 class="w-16 h-16 rounded" background-color="rgb(255, 105, 180)"
    }

    +text l3 content="HSL colors" class="text-zinc-400"
    +view hsl class="flex flex-row gap-2" {
      +view s1 class="w-16 h-16 rounded" background-color="hsl(0, 100%%, 50%%)"
      +view s2 class="w-16 h-16 rounded" background-color="hsl(120, 100%%, 50%%)"
      +view s3 class="w-16 h-16 rounded" background-color="hsl(240, 100%%, 50%%)"
      +view s4 class="w-16 h-16 rounded" background-color="hsl(300, 100%%, 50%%)"
      +view s5 class="w-16 h-16 rounded" background-color="hsl(60, 100%%, 50%%)"
    }

    +text l4 content="Named CSS colors" class="text-zinc-400"
    +view named class="flex flex-row gap-2" {
      +view n1 class="w-16 h-16 rounded" background-color=tomato
      +view n2 class="w-16 h-16 rounded" background-color=turquoise
      +view n3 class="w-16 h-16 rounded" background-color=gold
      +view n4 class="w-16 h-16 rounded" background-color=mediumpurple
      +view n5 class="w-16 h-16 rounded" background-color=hotpink
      +view n6 class="w-16 h-16 rounded" background-color=coral
      +view n7 class="w-16 h-16 rounded" background-color=dodgerblue
    }

    +text l5 content="Individual prop overrides class" class="text-zinc-400"
    +view override class="flex flex-row gap-2" {
      +view o1 class="w-16 h-16 rounded bg-red-500" background-color="#00ff88"
      +view o2 class="w-16 h-16 rounded bg-blue-500" background-color=coral
    }
  }
\e\\'

sleep 99999
