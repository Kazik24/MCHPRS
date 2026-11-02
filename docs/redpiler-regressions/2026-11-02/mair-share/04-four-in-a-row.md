# Four In A Row: pył zachowuje starą moc

**Potwierdzone w MAIR i Redpilerze. Najkrótszy z tych czterech przypadków do odtworzenia.**

Schemat: [Four In A Row Detector](../evidence/fixtures/b6a6e870229ace7eb2afd8cb387a514e-Four_In_A_Row_Detector.schem), ID `b6a6e870229ace7eb2afd8cb387a514e`.

## Odtworzenie

1. Wklej od `(8,8,8)`. Nie zmieniaj zapisanych mocy pyłu i nie czekaj przed pierwszą akcją.
2. Przełącz dźwignię `(8,9,27)` i wykonaj 20 ticków.
3. Przełącz dźwignię `(10,9,27)` i wykonaj dwa ticki.
4. Sprawdź odcinek pyłu `(12,9,10)`–`(12,9,13)`.

## Wynik

| Pył | Zwykła symulacja MAIR | Skompilowany MAIR |
| --- | ---: | ---: |
| `(12,9,10)` | 0 | 1 |
| `(12,9,11)` | 0 | 2 |
| `(12,9,12)` | 0 | 3 |
| `(12,9,13)` | 0 | 4 |

MCHPRS/Redpiler daje te same różnice. Bezpośrednio po drugim przełączeniu i po pierwszym ticku wszystko jeszcze się zgadza. Pierwsza różnica jest w drugim ticku, łącznie tick 22.

## Co znaleźliśmy u nas

W drugim ticku gaśnie pochodnia `(10,9,23)`. Powiadamia pył `(10,9,22)`, który wcześniej miał moc 15. Zwykła symulacja przelicza potem dalszy pył i usuwa starą moc 1–4. Redpiler nie odświeża tego dalekiego odcinka. Dla `(12,9,10)` zna już wejście 0, lecz nadal pokazuje zapisaną moc 1.

[Pełna lista działań](../evidence/mair/b6a6e870229ace7eb2afd8cb387a514e.actions.json) · [Wynik MAIR](../evidence/mair/b6a6e870229ace7eb2afd8cb387a514e.final.json) · [Dłuższy raport i logi](../four-in-a-row.md)
