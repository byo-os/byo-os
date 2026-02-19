#!/bin/bash
# Physics spring vs analytical spring — split-screen comparison
#
# Both sides chase the same random target every 400ms.
# - Left (green): physics-spring — preserves velocity on retarget → smooth
# - Right (magenta): analytical spring — restarts from zero velocity → jerky

printf '\e_B
  +view root class="flex w-full h-full bg-zinc-900" {
    +view left class="relative flex-1 overflow-hidden" {
      +text ltitle content="physics-spring" class="absolute top-4 left-1/2 translate-x-[-50%%] text-lg text-lime-400"
      +text lsub content="velocity preserved" class="absolute top-10 left-1/2 translate-x-[-50%%] text-xs text-zinc-500"

      +view pt6 class="absolute top-1/2 left-1/2 w-16 h-16 rounded-full bg-lime-400/5"
        transition="translate-x physics-spring(2,0.8), translate-y physics-spring(2,0.8)"
      +view pt5 class="absolute top-1/2 left-1/2 w-16 h-16 rounded-full bg-lime-400/8"
        transition="translate-x physics-spring(4,1.2), translate-y physics-spring(4,1.2)"
      +view pt4 class="absolute top-1/2 left-1/2 w-16 h-16 rounded-full bg-lime-400/12"
        transition="translate-x physics-spring(7,1.6), translate-y physics-spring(7,1.6)"
      +view pt3 class="absolute top-1/2 left-1/2 w-16 h-16 rounded-full bg-lime-400/16"
        transition="translate-x physics-spring(10,2), translate-y physics-spring(10,2)"
      +view pt2 class="absolute top-1/2 left-1/2 w-16 h-16 rounded-full bg-lime-400/20"
        transition="translate-x physics-spring(14,2.5), translate-y physics-spring(14,2.5)"
      +view pt1 class="absolute top-1/2 left-1/2 w-16 h-16 rounded-full bg-lime-400/30"
        transition="translate-x physics-spring(18,3), translate-y physics-spring(18,3)"
      +view pball class="absolute top-1/2 left-1/2 w-16 h-16 rounded-full bg-lime-400"
        transition="translate-x physics-spring(20,3), translate-y physics-spring(20,3)"
    }

    +view divider class="w-px bg-zinc-700"

    +view right class="relative flex-1 overflow-hidden" {
      +text rtitle content="spring (analytical)" class="absolute top-4 left-1/2 translate-x-[-50%%] text-lg text-fuchsia-400"
      +text rsub content="restarts each retarget" class="absolute top-10 left-1/2 translate-x-[-50%%] text-xs text-zinc-500"

      +view at6 class="absolute top-1/2 left-1/2 w-16 h-16 rounded-full bg-fuchsia-400/5"
        transition="translate-x spring(2,0.8), translate-y spring(2,0.8)"
      +view at5 class="absolute top-1/2 left-1/2 w-16 h-16 rounded-full bg-fuchsia-400/8"
        transition="translate-x spring(4,1.2), translate-y spring(4,1.2)"
      +view at4 class="absolute top-1/2 left-1/2 w-16 h-16 rounded-full bg-fuchsia-400/12"
        transition="translate-x spring(7,1.6), translate-y spring(7,1.6)"
      +view at3 class="absolute top-1/2 left-1/2 w-16 h-16 rounded-full bg-fuchsia-400/16"
        transition="translate-x spring(10,2), translate-y spring(10,2)"
      +view at2 class="absolute top-1/2 left-1/2 w-16 h-16 rounded-full bg-fuchsia-400/20"
        transition="translate-x spring(14,2.5), translate-y spring(14,2.5)"
      +view at1 class="absolute top-1/2 left-1/2 w-16 h-16 rounded-full bg-fuchsia-400/30"
        transition="translate-x spring(18,3), translate-y spring(18,3)"
      +view aball class="absolute top-1/2 left-1/2 w-16 h-16 rounded-full bg-fuchsia-400"
        transition="translate-x spring(20,3), translate-y spring(20,3)"
    }
  }
\e\\'
sleep 1

while true; do
    tx=$(( (RANDOM % 401) - 200 ))
    ty=$(( (RANDOM % 301) - 150 ))
    printf '\e_B
      @view pball translate-x="%d" translate-y="%d"
      @view pt1 translate-x="%d" translate-y="%d"
      @view pt2 translate-x="%d" translate-y="%d"
      @view pt3 translate-x="%d" translate-y="%d"
      @view pt4 translate-x="%d" translate-y="%d"
      @view pt5 translate-x="%d" translate-y="%d"
      @view pt6 translate-x="%d" translate-y="%d"
      @view aball translate-x="%d" translate-y="%d"
      @view at1 translate-x="%d" translate-y="%d"
      @view at2 translate-x="%d" translate-y="%d"
      @view at3 translate-x="%d" translate-y="%d"
      @view at4 translate-x="%d" translate-y="%d"
      @view at5 translate-x="%d" translate-y="%d"
      @view at6 translate-x="%d" translate-y="%d"
    \e\\' "$tx" "$ty" "$tx" "$ty" "$tx" "$ty" "$tx" "$ty" "$tx" "$ty" "$tx" "$ty" "$tx" "$ty" \
         "$tx" "$ty" "$tx" "$ty" "$tx" "$ty" "$tx" "$ty" "$tx" "$ty" "$tx" "$ty" "$tx" "$ty"
    sleep 0.4
done
