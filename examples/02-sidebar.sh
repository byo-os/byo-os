#!/bin/bash
# Sidebar layout — flex row with a fixed sidebar and flexible content area
printf '\e_B
  +view root class="flex flex-row w-full h-full" {
    +view sidebar class="flex flex-col w-64 h-full bg-zinc-800 p-4 gap-2" {
      +text title content="Navigation" class="text-white"
      +view item1 class="px-4 py-2 rounded bg-zinc-700" {
        +text item1-label content="Dashboard" class="text-zinc-200"
      }
      +view item2 class="px-4 py-2 rounded bg-blue-600" {
        +text item2-label content="Settings" class="text-white"
      }
      +view item3 class="px-4 py-2 rounded bg-zinc-700" {
        +text item3-label content="Profile" class="text-zinc-200"
      }
    }
    +view content class="flex flex-col grow p-8 bg-zinc-900 gap-4" {
      +text heading content="Welcome back!" class="text-2xl text-white"
      +text subtitle content="Select an item from the sidebar to get started." class="text-zinc-400"
    }
  }
\e\\'

sleep infinity
