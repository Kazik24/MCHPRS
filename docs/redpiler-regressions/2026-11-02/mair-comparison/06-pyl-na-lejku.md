# ANPU: zwykła symulacja MAIR usuwa pył na lejku

**Problem warstwy zwykłej symulacji MAIR. W naszym interpreterze pył zostaje na miejscu.**

Schemat: [ANPU](../evidence/fixtures/Q2CK@Q2CK_Anpu1_Pong_KBTV.schem), od `(8,8,8)`.

## Odtworzenie

1. Załaduj schemat z oryginalnymi stanami bloków.
2. Uruchom kontrolkę `(130,76,64)`.
3. Wykonaj58 ticków.
4. Sprawdź lejek `(60,35,15)` i pył nad nim, `(60,36,15)`.

## Wynik

Zwykła symulacja MAIR zamienia pył w powietrze. Skompilowana wersja zachowuje tam pył z mocą 0. Nasza zwykła symulacja również zachowuje pył.

Lejek został wczytany poprawnie. Problem znaleźliśmy przy sprawdzaniu, czy jego góra podpiera pył: `BlockInfo.SturdyTop` oraz `Wire.cs::NeighborChanged`.

Diagnostyczne ustawienie flagi podparcia lejka zatrzymuje usuwanie pyłu, ale odsłania późniejszy, osobny [błąd połączeń w ticku 278](05-anpu-brak-polaczenia.md). To nie jest dowód, że pozostała część ANPU działa poprawnie.

[Oryginalny wynik MAIR](../evidence/mair-anpu/result.json) · [Zapisany test](../evidence/mair-anpu/Program.cs) · [Dłuższa historia porównania](../../../REDPILER_NATIVE_BACKEND_EXPERIMENT.md)
