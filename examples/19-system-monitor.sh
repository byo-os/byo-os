#!/bin/bash
# System monitor — animated meters with dynamic color changes inside a layer
printf '\e_B
  +window main {
    +layer hud width=1280 height=720 {
      +view root class="flex flex-col w-full h-full bg-zinc-950 p-8" {
        +view header class="flex flex-row justify-between items-center" {
          +text title content="System Monitor" class="text-2xl text-white"
          +text clock content="00:00:00" class="text-zinc-500"
        }
        +view divider class="w-full h-px bg-zinc-800"
        +view meters class="flex flex-col gap-4 p-4" {
          +view cpu-row class="flex flex-col gap-1" {
            +text cpu-label content="CPU" class="text-zinc-400"
            +view cpu-track class="w-full h-4 rounded-full bg-zinc-800" {
              +view cpu-fill class="h-4 rounded-full bg-sky-500 transition-all duration-300" width=0
            }
            +text cpu-pct content="0%%" class="text-sky-400"
          }
          +view mem-row class="flex flex-col gap-1" {
            +text mem-label content="Memory" class="text-zinc-400"
            +view mem-track class="w-full h-4 rounded-full bg-zinc-800" {
              +view mem-fill class="h-4 rounded-full bg-emerald-500 transition-all duration-300" width=0
            }
            +text mem-pct content="0%%" class="text-emerald-400"
          }
          +view disk-row class="flex flex-col gap-1" {
            +text disk-label content="Disk I/O" class="text-zinc-400"
            +view disk-track class="w-full h-4 rounded-full bg-zinc-800" {
              +view disk-fill class="h-4 rounded-full bg-amber-500 transition-all duration-300" width=0
            }
            +text disk-pct content="0%%" class="text-amber-400"
          }
        }
        +text status content="Monitoring..." class="text-zinc-600"
      }
    }
  }
\e\\'
sleep 1

# Animate metrics
CPU=0; MEM=30; DISK=0
CPU_DIR=1; DISK_DIR=1

while true; do
    # Simulate fluctuating metrics
    CPU=$((CPU + CPU_DIR * (RANDOM % 8 + 1)))
    if [ $CPU -ge 95 ]; then CPU=95; CPU_DIR=-1; fi
    if [ $CPU -le 5 ]; then CPU=5; CPU_DIR=1; fi

    MEM=$((MEM + (RANDOM % 3) - 1))
    if [ $MEM -gt 80 ]; then MEM=80; fi
    if [ $MEM -lt 20 ]; then MEM=20; fi

    DISK=$((DISK + DISK_DIR * (RANDOM % 15 + 1)))
    if [ $DISK -ge 100 ]; then DISK=100; DISK_DIR=-1; fi
    if [ $DISK -le 0 ]; then DISK=0; DISK_DIR=1; fi

    TIME=$(date +%H:%M:%S)

    # Color changes based on thresholds
    if [ $CPU -ge 80 ]; then
        CPU_COLOR="bg-red-500"
    elif [ $CPU -ge 50 ]; then
        CPU_COLOR="bg-amber-500"
    else
        CPU_COLOR="bg-sky-500"
    fi

    printf '\e_B
      @text clock content="%s"
      @view cpu-fill class="h-4 rounded-full %s transition-all duration-300" width="%s%%"
      @text cpu-pct content="%d%%"
      @view mem-fill width="%s%%"
      @text mem-pct content="%d%%"
      @view disk-fill width="%s%%"
      @text disk-pct content="%d%%"
    \e\\' "$TIME" "$CPU_COLOR" "$CPU" "$CPU" "$MEM" "$MEM" "$DISK" "$DISK"

    sleep 0.3
done
