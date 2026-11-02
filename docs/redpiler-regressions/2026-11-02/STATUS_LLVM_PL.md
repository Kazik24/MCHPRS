# Status LLVM — 2 listopada 2026

**LLVM działa i zgadza się z naszym silnikiem Rust w wykonanych testach. Nie oznacza to, że wszystkie układy są już poprawne względem zwykłej symulacji.**

Wyniki dotyczą badanej wersji `ea297c16` i DLL o wersji semantycznej 19. Nie obejmują późniejszych zmian w bieżącym katalogu roboczym.

Naprawione są: kolejność powiadamiania tłoków przez pochodnię, kolejność obserwatorów przy pustym tłoku, przechowywanie i odczyt boków pyłu w LLVM oraz błąd kolejności pochodni w 8-bit RAM. Biblioteka ma wersję semantyczną **19**.

Co przeszło:

- Ekran ZCore zgadza się ze zwykłą symulacją w każdym z 5000 ticków, osobno dla Rust i LLVM.
- Rust i LLVM zgadzają się przez 5000 ticków ANPU i ZCore: stany bloków, kolejka ticków, ruch i sprawdzane efekty.
- Przejście z ANPU po kompilacji z powrotem do zwykłej symulacji daje ten sam wynik dla obu silników: 2000 ticków pracy i 128 ticków dalszej symulacji.
- Cztery problematyczne schematy z raportów dają w LLVM te same stany i kolejki co Rust przez całe zapisane sekwencje. Oba silniki nadal mają opisane błędy względem zwykłej symulacji.

## Szybkość

Mediana trzech par pomiarów, po 5000 ticków:

| Układ | Rust | LLVM |
| --- | ---: | ---: |
| ZCore | 2502 TPS | **4436 TPS** |
| ANPU | 19902 TPS | **34064 TPS** |

To czas samego wykonywania ticków. Wczytanie, kompilacja i publikowanie stanu są poza tym pomiarem. Nie jest to gwarancja takiej szybkości w każdym uruchomieniu.

## Co blokuje uznanie całości za gotową

Cztery błędy schematów nadal nie są naprawione. Pozostają też odłożone problemy zmian połączeń pyłu przy ruchu tłoków i dokładnej kolejności w ANPU. Ostatni pełny zestaw testów Redpilera miał 181 poprawnych, 3 niepoprawne i 18 pominiętych testów. Jeden z trzech niepoprawnych testów zbiera cztery błędy schematów. Późniejsze dodatkowe testy zgodności Rust/LLVM przeszły, ale nie usuwają tych błędów.

Testowaliśmy wykonanie DLL na Windows. Nie potwierdziliśmy działania tej wersji na Linuksie.

[Pełne wyniki i komendy](LLVM_STATUS.md) · [Porównanie z MAIR](mair-comparison/README.md) · [Zachowane logi i sumy kontrolne](EVIDENCE.md)
