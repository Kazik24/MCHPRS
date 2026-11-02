# Carry-cancel 8111: Java 1.21.5 potwierdza moc 0

**Vanilla Java 1.21.5 rozstrzyga widoczny wynik akcji 39: pył `(14,9,8)` ma moc 0 w ticku 5.** Zwykła symulacja MCHPRS zwraca 1; Redpiler Rust/LLVM i obie wersje MAIR zwracają 0.

Schemat: [Eight-bit carry-cancel adder](../evidence/fixtures/8111b000090d8f0b3e1370448bcb64a3-8_bit_Carry_Cancel_Adder.schem), ID `8111b000090d8f0b3e1370448bcb64a3`.

## Odtworzenie

1. Wklej schemat od `(8,8,8)`, bez dodatkowego czekania.
2. Wykonaj [dokładną listę działań](../evidence/mair/8111b000090d8f0b3e1370448bcb64a3.actions.json): 66 działań i 831 ticków.
3. Przy akcji 39 przełącz dźwignię `(14,10,28)` i sprawdź tick 5. To łącznie tick 699.
4. Sprawdź moc pyłu w `(14,9,8)`.

## Wynik

| Silnik | Moc pyłu w ticku 5 |
| --- | ---: |
| Zwykła symulacja MCHPRS | 1 |
| Redpiler Rust i LLVM | 0 |
| Zwykła symulacja MAIR | 0 |
| Skompilowany MAIR | 0 |
| Vanilla Java 1.21.5 | **0** |

Java zgadza się z Redpilerem i MAIR, nie ze zwykłą symulacją MCHPRS. To nie jest błąd samego kompilatora Redpilera: rozbieżność powstaje między interpreterem MCHPRS a wykonaniem Java.

## Ślad z Java

Zrzut używa [oficjalnego serwera Mojang Java 1.21.5](https://piston-data.mojang.com/v1/objects/e6ec2f64e6080b9b5d9b471b291c33cc7f509733/server.jar), SHA-1 `e6ec2f64e6080b9b5d9b471b291c33cc7f509733`, SHA-256 `ae7681dadce21b6b4017d28e7eb567d86b6c100a6969994f540b9e54f812dc29`. Schemat i jego block entities wczytano z zapisanych stanów przez vanilla `/setblock` i `/data merge block`; nie dodano ticków na osiadanie. Sekwencja wejść i pozycje odpowiadają zapisanym danym. Dźwignie przełącza klient offline wysyłający zwykły pakiet `UseItemOn`. Serwer jest zamrożony; po każdym `tick step 1` narzędzie czeka, aż licznik czasu świata wzrośnie, zanim wykona następną akcję.

| Lokalny tick akcji 39 | Wyjście komparatora `(10,10,11)` | Wyjście komparatora `(13,9,9)` | Moc pyłu `(14,9,8)` |
| ---: | ---: | ---: | ---: |
| przed przełączeniem | 7 | 0 | 1 |
| 0 | 7 | 0 | 1 |
| 1 | 7 | 0 | 1 |
| 2 | 7 | 0 | 1 |
| 3 | 0 | 0 | 1 |
| 4 | 0 | 0 | 1 |
| 5 | 0 | 0 | **0** |

Java odtwarza spadek wyjścia źródłowego w ticku 3 i kończy z mocą pyłu 0 w ticku 5. Nasza zwykła symulacja w tym samym miejscu raportuje krótkie wejście tylne 3/boczne 0 i planuje tick komparatora, po którym wynik wynosi 1. Ten wewnętrzny callback nie daje jednak zachowania widocznego w vanilla. Zapisany ślad nie udostępnia kolejki ani kolejności callbacków Java, ale rozstrzyga wynik pyłu.

## Artefakty

- [Wynik Java 1.21.5](../evidence/java-8111-carry-cancel.json)
- [Skrypt przechwytywania i synchronizacji ticków](../evidence/java-8111-capture.py)
- [Pełny wynik MAIR](../evidence/mair/8111b000090d8f0b3e1370448bcb64a3.final.json) · [stan źródeł i kolejki MAIR](../evidence/mair/8111b000090d8f0b3e1370448bcb64a3.source-result.json)
- [Dłuższy raport MCHPRS z logami](../carry-cancel-action-39.md)
