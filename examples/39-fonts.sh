#!/bin/bash
# Example 39: Font Book
# A visual catalog of all bundled font families, weights, and styles.
# Auto-scrolls the specimen sheet and includes a live TTY specimen.

ESC=$'\e'
APC="${ESC}_B"
ST="${ESC}\\"

SAMPLE="The quick brown fox jumps over the lazy dog. 0123456789"

# Discard passthrough to root tty so it doesn't show through
printf "${APC} #redirect _ ${ST}"

printf "${APC}
  +view root class=\"w-full h-full bg-zinc-900 overflow-clip\" {
    +view scroll-inner class=\"absolute w-full flex flex-col gap-6 p-8\" translate-y=0 transition=\"translate-y 20000ms linear\" {

      +text title content=\"Font Book\" class=\"text-4xl text-white font-bold\"
      +text subtitle content=\"10 bundled families  /  49 variants\" class=\"text-base text-zinc-500\"

      +view sep0 class=\"h-px bg-zinc-800 w-full\"

      +view fam-vendsans class=\"flex flex-col gap-1\" {
        +text fam-vendsans-name content=\"Vend Sans  —  system-ui  (default)\" class=\"text-sm text-emerald-400 font-mono\"
        +text fam-vendsans-light content=\"Light 300: ${SAMPLE}\" class=\"text-xl text-zinc-200 font-light\" font-family=\"Vend Sans\"
        +text fam-vendsans-reg content=\"Regular 400: ${SAMPLE}\" class=\"text-xl text-zinc-200\" font-family=\"Vend Sans\"
        +text fam-vendsans-med content=\"Medium 500: ${SAMPLE}\" class=\"text-xl text-zinc-200 font-medium\" font-family=\"Vend Sans\"
        +text fam-vendsans-semi content=\"SemiBold 600: ${SAMPLE}\" class=\"text-xl text-zinc-200 font-semibold\" font-family=\"Vend Sans\"
        +text fam-vendsans-bold content=\"Bold 700: ${SAMPLE}\" class=\"text-xl text-zinc-200 font-bold\" font-family=\"Vend Sans\"
        +text fam-vendsans-it content=\"Italic 400: ${SAMPLE}\" class=\"text-xl text-zinc-200 italic\" font-family=\"Vend Sans\"
        +text fam-vendsans-boldit content=\"Bold Italic 700: ${SAMPLE}\" class=\"text-xl text-zinc-200 font-bold italic\" font-family=\"Vend Sans\"
      }

      +view sep1 class=\"h-px bg-zinc-800 w-full\"

      +view fam-libsans class=\"flex flex-col gap-1\" {
        +text fam-libsans-name content=\"Liberation Sans  —  sans-serif\" class=\"text-sm text-emerald-400 font-mono\"
        +text fam-libsans-reg content=\"Regular 400: ${SAMPLE}\" class=\"text-xl text-zinc-200 font-sans\"
        +text fam-libsans-bold content=\"Bold 700: ${SAMPLE}\" class=\"text-xl text-zinc-200 font-sans font-bold\"
        +text fam-libsans-it content=\"Italic 400: ${SAMPLE}\" class=\"text-xl text-zinc-200 font-sans italic\"
        +text fam-libsans-boldit content=\"Bold Italic 700: ${SAMPLE}\" class=\"text-xl text-zinc-200 font-sans font-bold italic\"
      }

      +view sep2 class=\"h-px bg-zinc-800 w-full\"

      +view fam-libserif class=\"flex flex-col gap-1\" {
        +text fam-libserif-name content=\"Liberation Serif  —  serif\" class=\"text-sm text-emerald-400 font-mono\"
        +text fam-libserif-reg content=\"Regular 400: ${SAMPLE}\" class=\"text-xl text-zinc-200 font-serif\"
        +text fam-libserif-bold content=\"Bold 700: ${SAMPLE}\" class=\"text-xl text-zinc-200 font-serif font-bold\"
        +text fam-libserif-it content=\"Italic 400: ${SAMPLE}\" class=\"text-xl text-zinc-200 font-serif italic\"
        +text fam-libserif-boldit content=\"Bold Italic 700: ${SAMPLE}\" class=\"text-xl text-zinc-200 font-serif font-bold italic\"
      }

      +view sep3 class=\"h-px bg-zinc-800 w-full\"

      +view fam-libmono class=\"flex flex-col gap-1\" {
        +text fam-libmono-name content=\"Liberation Mono  —  monospace\" class=\"text-sm text-emerald-400 font-mono\"
        +text fam-libmono-reg content=\"Regular 400: ${SAMPLE}\" class=\"text-xl text-zinc-200 font-mono\"
        +text fam-libmono-bold content=\"Bold 700: ${SAMPLE}\" class=\"text-xl text-zinc-200 font-mono font-bold\"
        +text fam-libmono-it content=\"Italic 400: ${SAMPLE}\" class=\"text-xl text-zinc-200 font-mono italic\"
        +text fam-libmono-boldit content=\"Bold Italic 700: ${SAMPLE}\" class=\"text-xl text-zinc-200 font-mono font-bold italic\"
      }

      +view sep4 class=\"h-px bg-zinc-800 w-full\"

      +view fam-firacode class=\"flex flex-col gap-1\" {
        +text fam-firacode-name content=\"Fira Code  —  code\" class=\"text-sm text-emerald-400 font-mono\"
        +text fam-firacode-light content=\"Light 300: => != === >= ++ -- :: ->\" class=\"text-xl text-zinc-200 font-light font-code\"
        +text fam-firacode-reg content=\"Regular 400: => != === >= ++ -- :: ->\" class=\"text-xl text-zinc-200 font-code\"
        +text fam-firacode-med content=\"Medium 500: => != === >= ++ -- :: ->\" class=\"text-xl text-zinc-200 font-medium font-code\"
        +text fam-firacode-semi content=\"SemiBold 600: => != === >= ++ -- :: ->\" class=\"text-xl text-zinc-200 font-semibold font-code\"
        +text fam-firacode-bold content=\"Bold 700: => != === >= ++ -- :: ->\" class=\"text-xl text-zinc-200 font-bold font-code\"
      }

      +view sep5 class=\"h-px bg-zinc-800 w-full\"

      +view fam-firamono class=\"flex flex-col gap-1\" {
        +text fam-firamono-name content=\"Fira Mono  —  ui-monospace  (tty default)\" class=\"text-sm text-emerald-400 font-mono\"
        +text fam-firamono-reg content=\"Regular 400: ${SAMPLE}\" class=\"text-xl text-zinc-200 font-ui-mono\"
        +text fam-firamono-med content=\"Medium 500: ${SAMPLE}\" class=\"text-xl text-zinc-200 font-ui-mono font-medium\"
        +text fam-firamono-bold content=\"Bold 700: ${SAMPLE}\" class=\"text-xl text-zinc-200 font-ui-mono font-bold\"
        +tty term1 class=\"w-full rows-12 bg-zinc-950 rounded-lg p-1\" font-family=\"Fira Code\"
      }

      +view sep6 class=\"h-px bg-zinc-800 w-full\"

      +view fam-comfortaa class=\"flex flex-col gap-1\" {
        +text fam-comfortaa-name content=\"Comfortaa  —  ui-rounded\" class=\"text-sm text-emerald-400 font-mono\"
        +text fam-comfortaa-light content=\"Light 300: ${SAMPLE}\" class=\"text-xl text-zinc-200 font-light font-ui-rounded\"
        +text fam-comfortaa-reg content=\"Regular 400: ${SAMPLE}\" class=\"text-xl text-zinc-200 font-ui-rounded\"
        +text fam-comfortaa-med content=\"Medium 500: ${SAMPLE}\" class=\"text-xl text-zinc-200 font-medium font-ui-rounded\"
        +text fam-comfortaa-semi content=\"SemiBold 600: ${SAMPLE}\" class=\"text-xl text-zinc-200 font-semibold font-ui-rounded\"
        +text fam-comfortaa-bold content=\"Bold 700: ${SAMPLE}\" class=\"text-xl text-zinc-200 font-bold font-ui-rounded\"
      }

      +view sep7 class=\"h-px bg-zinc-800 w-full\"

      +view fam-dmserif class=\"flex flex-col gap-1\" {
        +text fam-dmserif-name content=\"DM Serif Text  —  ui-serif\" class=\"text-sm text-emerald-400 font-mono\"
        +text fam-dmserif-reg content=\"Regular 400: ${SAMPLE}\" class=\"text-2xl text-zinc-200 font-ui-serif\"
        +text fam-dmserif-it content=\"Italic 400: ${SAMPLE}\" class=\"text-2xl text-zinc-200 font-ui-serif italic\"
      }

      +view sep8 class=\"h-px bg-zinc-800 w-full\"

      +view fam-felipa class=\"flex flex-col gap-1\" {
        +text fam-felipa-name content=\"Felipa  —  cursive\" class=\"text-sm text-emerald-400 font-mono\"
        +text fam-felipa-reg content=\"${SAMPLE}\" class=\"text-3xl text-zinc-200 font-cursive\"
      }

      +view sep9 class=\"h-px bg-zinc-800 w-full\"

      +view fam-tangerine class=\"flex flex-col gap-1\" {
        +text fam-tangerine-name content=\"Tangerine  —  fantasy\" class=\"text-sm text-emerald-400 font-mono\"
        +text fam-tangerine-reg content=\"Regular: ${SAMPLE}\" class=\"text-4xl text-zinc-200 font-fantasy\"
        +text fam-tangerine-bold content=\"Bold: ${SAMPLE}\" class=\"text-4xl text-zinc-200 font-fantasy font-bold\"
      }

      +view pad class=\"h-96\"
    }
  }
${ST}"

# Wait for compositor to start and the TTY entity to exist in the IdMap
# before redirecting passthrough to it.
sleep 5

# Feed the TTY specimen
printf "${APC} #redirect term1 ${ST}"
sleep 0.1
printf "\e[1;32m$ \e[0mneofetch --ascii\r\n"
sleep 0.15
printf "\e[1;36m  ____  __   __ ___    ___  ____\r\n"
printf " | __ ) \\ \\ / // _ \\  / _ \\/ ___|\r\n"
printf " |  _ \\  \\ V /| | | || | | \\___ \\\\\r\n"
printf " | |_) |  | | | |_| || |_| |___) |\r\n"
printf " |____/   |_|  \\___/  \\___/|____/\r\n"
printf "\e[0m\r\n"
sleep 0.1
printf "\e[1;37mOS:\e[0m       BYO/OS 0.1\r\n"
printf "\e[1;37mShell:\e[0m    bash 5.2\r\n"
printf "\e[1;37mTerminal:\e[0m byo-compositor\r\n"
printf "\e[1;37mFont:\e[0m     Fira Code\r\n"
printf "\e[1;37mFamilies:\e[0m 10 bundled (49 variants)\r\n"
printf "\r\n\e[1;32m$ \e[0m"

# Stop redirecting
printf "${APC} #redirect _ ${ST}"

# Auto-scroll the specimen sheet
sleep 3
while true; do
    # Scroll down
    printf "${APC} @view scroll-inner translate-y=-1200 ${ST}"
    sleep 24

    # Scroll back to top
    printf "${APC} @view scroll-inner translate-y=0 ${ST}"
    sleep 24
done
