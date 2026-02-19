#!/bin/bash
# Box shadows — Tailwind presets, color overrides, and CSS syntax
printf '\e_B
  +view root class="flex flex-col items-center p-6 gap-5 w-full h-full bg-zinc-200" {
    +text title content="Box Shadows" class="text-2xl text-zinc-800"

    +text l1 content="Tailwind presets" class="text-zinc-500"
    +view presets class="flex flex-row gap-4 items-center p-4" {
      +view s-sm class="flex items-center justify-center w-24 h-14 rounded-lg bg-white shadow-sm" {
        +text t-sm content="sm" class="text-sm text-zinc-600"
      }
      +view s-def class="flex items-center justify-center w-24 h-14 rounded-lg bg-white shadow" {
        +text t-def content="shadow" class="text-sm text-zinc-600"
      }
      +view s-md class="flex items-center justify-center w-24 h-14 rounded-lg bg-white shadow-md" {
        +text t-md content="md" class="text-sm text-zinc-600"
      }
      +view s-lg class="flex items-center justify-center w-24 h-14 rounded-lg bg-white shadow-lg" {
        +text t-lg content="lg" class="text-sm text-zinc-600"
      }
      +view s-xl class="flex items-center justify-center w-24 h-14 rounded-lg bg-white shadow-xl" {
        +text t-xl content="xl" class="text-sm text-zinc-600"
      }
      +view s-2xl class="flex items-center justify-center w-24 h-14 rounded-lg bg-white shadow-2xl" {
        +text t-2xl content="2xl" class="text-sm text-zinc-600"
      }
    }

    +text l2 content="Colored shadows" class="text-zinc-500"
    +view colors class="flex flex-row gap-4 items-center p-4" {
      +view sc1 class="w-16 h-16 rounded-xl bg-blue-500 shadow-lg shadow-blue-500"
      +view sc2 class="w-16 h-16 rounded-xl bg-emerald-500 shadow-lg shadow-emerald-500"
      +view sc3 class="w-16 h-16 rounded-xl bg-purple-500 shadow-lg shadow-purple-500"
      +view sc4 class="w-16 h-16 rounded-xl bg-rose-500 shadow-lg shadow-rose-500"
      +view sc5 class="w-16 h-16 rounded-xl bg-amber-500 shadow-lg shadow-amber-500"
    }

    +text l3 content="CSS box-shadow prop" class="text-zinc-500"
    +view custom class="flex flex-row gap-4 items-center p-4" {
      +view cs1 class="flex items-center justify-center w-24 h-14 rounded-lg bg-white" box-shadow="0 4px 14px rgba(0,0,0,0.25)" {
        +text ct1 content="deep" class="text-sm text-zinc-600"
      }
      +view cs2 class="flex items-center justify-center w-24 h-14 rounded-lg bg-white" box-shadow="4px 4px 0 rgba(0,0,0,0.2)" {
        +text ct2 content="hard" class="text-sm text-zinc-600"
      }
      +view cs3 class="flex items-center justify-center w-24 h-14 rounded-lg bg-white" box-shadow="0 0 20px rgba(59,130,246,0.5)" {
        +text ct3 content="glow" class="text-sm text-zinc-600"
      }
      +view cs4 class="flex items-center justify-center w-24 h-14 rounded-lg bg-white" box-shadow="0 8px 30px rgba(0,0,0,0.12), 0 2px 8px rgba(0,0,0,0.06)" {
        +text ct4 content="layered" class="text-sm text-zinc-600"
      }
    }

    +text l4 content="Shapes with shadows" class="text-zinc-500"
    +view shapes class="flex flex-row gap-6 items-center p-4" {
      +view sh1 class="w-16 h-16 rounded-full bg-sky-400 shadow-xl shadow-sky-400"
      +view sh2 class="w-24 h-12 rounded-full bg-pink-400 shadow-lg shadow-pink-400"
      +view sh3 class="w-14 h-14 rounded bg-violet-400 shadow-2xl shadow-violet-500"
    }
  }
\e\\'

sleep 99999
