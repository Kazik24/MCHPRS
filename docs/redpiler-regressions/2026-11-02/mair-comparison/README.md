# Porównanie MAIR i Redpilera

Poniżej są krótkie zgłoszenia z dokładnym sposobem odtworzenia. Pełne wyniki, schematy i listy przełączeń są załączone w katalogu `evidence`.

Raporty korzystają z załączników zapisanych w tym samym katalogu `2026-11-02`.

| Problem | MAIR | Nasz Redpiler |
| --- | --- | --- |
| [ANPU: połączenie odsłonięte przez kilka tłoków](05-anpu-brak-polaczenia.md) | Pomija źródło sygnału | Naprawione wykrywanie tego rodzaju połączeń; nie oznacza to naprawienia całego ANPU |
| [Pył na lejku](06-pyl-na-lejku.md) | Zwykła symulacja usuwa pył | Nasza zwykła symulacja zachowuje pył; to różnica interpretera, nie tylko kompilatora |
| [Carry-cancel 2cdc](02-carry-cancel-2cdc.md) | Błąd w akcji 43 | Nadal ten sam błąd |
| [Adder 2735](03-adder-2735.md) | Błąd w akcji 63 | Nadal ten sam błąd |
| [Four In A Row](04-four-in-a-row.md) | Błąd w akcji 1 | Nadal ten sam błąd |
| [Carry-cancel 8111](01-carry-cancel-8111.md) | Obie wersje zgadzają się ze sobą | U nas wersje się różnią; inaczej zachowują się też nasze zwykłe symulacje |

Nie zaliczamy ostatniego przypadku do potwierdzonych błędów MAIR. Nie zakładamy też, że każda nasza ostatnia poprawka była błędem MAIR. Dodatkowy test 8-bit RAM nie znalazł różnicy między dwiema wersjami MAIR przez 80 akcji i 1104 ticki: [wynik](../evidence/mair/7abf58b22e8e91c72ca27f70c1fd10b1.final.json).

## Jak czytać zgłoszenia

- Tick to jeden krok symulacji. Akcje liczymy od 0; akcja 43 jest 44. przełączeniem.
- Pozycje są współrzędnymi świata `(x,y,z)`. Schemat wklejamy od `(8,8,8)`.
- Nie czekamy przed pierwszą akcją i nie poprawiamy zapisanych stanów pyłu ani komparatorów.
- Pliki `actions.json` zawierają dokładne dźwignie i czas oczekiwania. Nie trzeba odtwarzać losowania.
- [Wspólna instrukcja](../README.md) i [sumy kontrolne](../EVIDENCE.md) pozwalają sprawdzić dane.

## Badana wersja

Biblioteka MAIR była niezmienioną odbudową z dekompilowanego kodu. Wersja `1.0.0.0`, SHA-256:
`1510a2596486a0df240750bc96c7b0f446697fdf9fe6f3039f9b73787c99d5ac`.

Oryginalnej wydanej biblioteki nie mieliśmy. Jeśli autor ma inną wersję, warto powtórzyć testy na niej. Zgodność dwóch wersji MAIR nie rozstrzyga automatycznie zgodności z Minecraftem.
