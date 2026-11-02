# Carry-cancel 8111: różnica między zwykłymi symulacjami

**To nie jest jeszcze potwierdzony błąd MAIR.**

Schemat: [Eight-bit carry-cancel adder](../evidence/fixtures/8111b000090d8f0b3e1370448bcb64a3-8_bit_Carry_Cancel_Adder.schem), ID `8111b000090d8f0b3e1370448bcb64a3`.

## Odtworzenie

1. Wklej schemat od `(8,8,8)`, bez dodatkowego czekania.
2. Wykonaj [dokładną listę działań](../evidence/mair/8111b000090d8f0b3e1370448bcb64a3.actions.json).
3. Przy akcji 39 przełącz dźwignię `(14,10,28)` i sprawdź tick 5. To łącznie tick 699.
4. Sprawdź moc pyłu w `(14,9,8)`.

## Wynik

| Wersja | Moc pyłu |
| --- | ---: |
| Zwykła symulacja MCHPRS | 1 |
| Nasz Redpiler Rust i LLVM | 0 |
| Zwykła symulacja MAIR | 0 |
| Skompilowany MAIR | 0 |

MAIR zgadza się ze sobą przez cały test:66 działań,831 ticków. Nie rozstrzygamy tutaj, która zwykła symulacja zachowuje się poprawnie.

## Co znaleźliśmy u nas

W ticku 3 komparator `(10,10,11)` zmienia wyjście z 7 na 0. Zasila komparator `(13,9,9)` jednocześnie od tyłu i z boku. Przez chwilę tylne wejście ma 3, a boczne już0. Nasza zwykła symulacja planuje wtedy tick komparatora na tick 5. Redpiler widzi dopiero końcowe0/0 i pomija ten krótki impuls; później planuje tick 6.

[Pełny wynik MAIR](../evidence/mair/8111b000090d8f0b3e1370448bcb64a3.final.json) · [Stan źródeł i kolejki](../evidence/mair/8111b000090d8f0b3e1370448bcb64a3.source-result.json) · [Dłuższy raport z logami](../carry-cancel-action-39.md)
