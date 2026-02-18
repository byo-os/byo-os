#!/bin/bash
# 3D carousel — layers arranged in a circle, continuously rotating
LAYERS=6
WIDTH=250
HEIGHT=180
RADIUS=300

# Spawn layers in a ring
printf '\e_B +window scene {\n'
for i in $(seq 0 $((LAYERS - 1))); do
    HUE=$(( i * 360 / LAYERS ))
    read -r TX TZ RY <<< $(awk "BEGIN {
        pi = 3.14159265;
        a = $i * 2 * pi / $LAYERS;
        printf \"%.1f %.1f %.1f\", $RADIUS * sin(a), $RADIUS * cos(a), -$i * 360 / $LAYERS
    }")
    # Pick a color per card
    COLORS=("sky" "emerald" "purple" "amber" "rose" "teal")
    C=${COLORS[$i]}
    printf '  +layer card%d width=%d height=%d order=%d
        translate-x="%s" translate-z="%s" rotate-y="%s"
        unlit=true alpha-mode=blend cull-mode=none {\n' \
        "$i" "$WIDTH" "$HEIGHT" "$i" "$TX" "$TZ" "$RY"
    printf '    +view c%d-root class="flex flex-col items-center justify-center w-full h-full bg-%s-800 rounded-xl border border-%s-600" {\n' "$i" "$C" "$C"
    printf '      +text c%d-title content="Card %d" class="text-2xl text-white"\n' "$i" "$((i + 1))"
    printf '      +text c%d-sub content="Layer %d of %d" class="text-%s-300"\n' "$i" "$((i + 1))" "$LAYERS" "$C"
    printf '    }\n  }\n'
done
printf '}\n\e\\'
sleep 0.5

# Animate the carousel rotation
T=0
while true; do
    for i in $(seq 0 $((LAYERS - 1))); do
        read -r TX TZ RY <<< $(awk "BEGIN {
            pi = 3.14159265;
            base_angle = $i * 2 * pi / $LAYERS;
            spin = $T * pi / 180;
            a = base_angle + spin;
            printf \"%.1f %.1f %.1f\", $RADIUS * sin(a), $RADIUS * cos(a), -(($i * 360 / $LAYERS) + $T)
        }")
        printf '\e_B @layer card%d translate-x="%s" translate-z="%s" rotate-y="%s" \e\\' \
            "$i" "$TX" "$TZ" "$RY"
    done

    T=$(( (T + 1) % 360 ))
done
