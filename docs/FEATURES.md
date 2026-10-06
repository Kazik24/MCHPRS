## MRAWW - Redstonefun's MCHPRS Fork

- MCHPRS został zportowany do wersji Minecraft 1.21.5
- Dodano wsparcie dla zwykłych, sticky i instant pistonów w interpreterze. Poprawiono zgodność ich działania z Minecraftem 1.21.5, w tym kolejność update'ów, krótkie impulsy i współpracę z observerami. Praktycznie każdy procesor działający na 1.21.5 powinien działać na MRAWW. Wyjątek to slimebloki.
- Dodano nanotick / picotick stepping - `/adv nano <count>` pozwala przechodzić przez partie operacji, a `/adv pico <count>` przez pojedyncze operacje symulacji. Przed krokowaniem zatrzymaj symulację przez `/tps 0`
- Dodano nagrywanie i cofanie symulacji - `/rhistory on [ticks]` zapisuje historię, a `/back [ticks]` cofa symulacje o zadaną ilość ticków. Feature dostępny dla I/E/Z, E/Z mają limit 100 ticków w wstecz (10s)
- Dodano wersjonowanie całych działek przez `/git` - zapisywanie wersji, branche, historia, przywracanie budowli i odzyskiwanie niezapisanych zmian sprzed zmiany wersji, dostęp od E. E ma limit pamięci.
- Dodano wizualne porównywanie wersji działki `/git diff <from> <to>` pokazuje zmienione bloki jako kolorowe, świecące znaczniki
- Dodano podgląd sąsiednich działek bez konieczności wchodzenia na nie.
- Dodano port redstonetoolsów: (`//find <maska>`, `/container`, `//signsearch <regex>` ect)
- Dodano featuery z plots^2 w tym `/p add <nick>` i `/p remove <nick>`, `/p home`, `/p visit <gracz>`
- Rozszerzono `//rstack` i dodano `/autostack [direction] [count] [spacing] [-e]`
- Dodano import schematów Sponge v2/v3 i poprawiono zachowywanie tabliczek, kontenerów oraz command blocków.
- Poprawiono WorldEdit - zaznaczanie wklejonego obszaru przez `//paste -s`, cofanie nakładających się operacji i obsługę łączonych flag
- Dodano `/say` i `/tellraw` oraz command blocki. Obsługiwane komendy command blocków to `say` i `tellraw`
- Rozszerzono obsługę bloków redstone np. drewnianych płytek naciskowych
- Dodano dźwięki stawiania i niszczenia bloków, przełączania dźwigni, przycisków i komparatorów oraz otwierania kontenerów. Poprawiono odtwarzanie note blocków
- Dodano obsługę rang, kolorowych prefiksów i uprawnień do budowania oraz korzystania z komend
- Dodano `/help`
- Dodano bardzo dużo optymalizacji silnika redstone - szybsze, cachowane rozchodzenie się sygnału po kablach, sprawniejszą obsługę wielu pistonów i kolejek update'ów oraz cachowanie informacji o blokach i ich sąsiadach. Zysk wydajności zależy od budowli, ale zazwyczaj jest to 10 - 15x dla bardzo dużych budowli (PM1), 100x dla średnich (ANPU) i do 200x dla małych.
