#!/bin/bash
# Simulated TODO app — builds up a list, checks items off, removes them
printf '\e_B
  +view root class="flex flex-col items-center w-full h-full bg-zinc-950 p-8" {
    +view card class="flex flex-col w-96 bg-zinc-900 rounded-2xl p-6 gap-4 border border-zinc-800" {
      +text title content="TODO List" class="text-2xl text-white"
      +view divider class="w-full h-px bg-zinc-700"
      +view list class="flex flex-col gap-2"
      +view divider2 class="w-full h-px bg-zinc-700"
      +text count content="0 items" class="text-zinc-500"
    }
  }
\e\\'
sleep 1

ITEMS=("Buy groceries" "Write tests" "Review PR" "Deploy to prod" "Update docs" "Fix bug #42")
COUNT=0

# Add items one by one
for ITEM in "${ITEMS[@]}"; do
    COUNT=$((COUNT + 1))
    ID="todo${COUNT}"
    printf '\e_B
      @view list {
        +view %s class="flex flex-row items-center gap-3 px-3 py-2 rounded-lg bg-zinc-800" {
          +view %s-check class="w-5 h-5 rounded border-2 border-zinc-500"
          +text %s-text content="%s" class="text-zinc-200"
        }
      }
      @text count content="%d items"
    \e\\' "$ID" "$ID" "$ID" "$ITEM" "$COUNT"
    sleep 0.5
done

sleep 2

# Check off items (change style to indicate done)
for i in 1 3 5; do
    ID="todo${i}"
    printf '\e_B
      @view %s-check class="w-5 h-5 rounded bg-emerald-500"
      @text %s-text class="text-zinc-500"
    \e\\' "$ID" "$ID"
    sleep 0.8
done

printf '\e_B @text count content="3 of 6 done" \e\\'
sleep 2

# Remove completed items
for i in 5 3 1; do
    ID="todo${i}"
    printf '\e_B -view %s \e\\' "$ID"
    sleep 0.5
done

printf '\e_B @text count content="3 items remaining" \e\\'
sleep infinity
