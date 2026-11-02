# Carry-cancel 2cdc: po kompilacji znika sygnał

**Potwierdzone w MAIR i Redpilerze. Naprawy jeszcze nie ma.**

Schemat: [8-bit Carry Cancel Adder](../evidence/fixtures/2cdc24e68c78d7f0cdce7a141648385a-8_bit_Carry_Cancel_Adder.schem), ID `2cdc24e68c78d7f0cdce7a141648385a`.

## Odtworzenie

1. Wklej od `(8,8,8)`, bez dodatkowego czekania.
2. Wykonaj [całą listę działań](../evidence/mair/2cdc24e68c78d7f0cdce7a141648385a.actions.json), nie tylko ostatnie przełączenie.
3. W akcji 43 przełącz dźwignię `(11,20,21)`, a potem wykonaj dwa ticki.
4. Już w pierwszym ticku sprawdź pył w `(11,15,9)`; to łącznie tick 717.

## Wynik

W MAIR zwykła symulacja daje moc **14**, a wersja skompilowana **0**. MCHPRS i Redpiler różnią się dokładnie w tym samym miejscu i czasie. Rust i LLVM zgadzają się ze sobą.

## Wcześniejszy problem znaleziony u nas

W akcji 18, po przełączeniu `(8,24,21)`, nasza zwykła symulacja planuje najpierw komparator `(10,24,18)`, potem pochodnię `(13,24,16)`. Redpiler robi odwrotnie.

Podczas układania programu odrzuca powiadomienie komparatora: dźwignia się wyłącza, ale wejścia komparatora nadal są włączone. Potem umieszcza go na końcu listy. To potwierdzony błąd kolejności u nas, ale nie sprawdziliśmy jeszcze, czy jego naprawa usunie późniejszą różnicę mocy. Nie potwierdziliśmy też tego samego mechanizmu wewnątrz MAIR.


[Pełny wynik MAIR](../evidence/mair/2cdc24e68c78d7f0cdce7a141648385a.final.json) · [Dłuższy raport i logi](../carry-cancel-action-43.md)
