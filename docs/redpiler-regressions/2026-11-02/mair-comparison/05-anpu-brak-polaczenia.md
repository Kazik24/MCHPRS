# ANPU: kompilator pomija połączenie odsłaniane przez tłoki

**Potwierdzone w badanym MAIR. U nas naprawiono wykrywanie tego rodzaju połączeń. To nie jest deklaracja pełnej poprawności ANPU.**

Schemat: [ANPU](../evidence/fixtures/Q2CK@Q2CK_Anpu1_Pong_KBTV.schem), SHA-256 `f4068257797399531ce31d56c972af32d0f73f7d8d496ce1b7b8cd1c178decec`.

## Co się rozjeżdża

W ticku **278** pył **`(99,49,109)`** ma moc **5** w zwykłej symulacji MAIR i **0** po kompilacji. W zapisanej tabeli źródeł brakuje pochodni **`(98,48,118)`**, która powinna zasilać ten pył w danym układzie tłoków.

## Ważny warunek testu

Najpierw trzeba ominąć osobny [problem pyłu na lejku](06-pyl-na-lejku.md). W naszym teście zmieniono tylko flagę podparcia lejka w pamięci procesu. Nie zmieniano kodu ani DLL MAIR. Bez tego osobny błąd pojawia się już w ticku 58.

1. Wklej ANPU od `(8,8,8)`.
2. Użyj kontrolki startu `(130,76,64)`.
3. Uruchom278 ticków i porównaj wskazany pył.

Gotowy test diagnostyczny: zapisany [Program.cs](../evidence/mair-anpu/Program.cs), uruchamiany z argumentami ścieżka-repozytorium, `4096`, `hopper-support`. [Projekt](../evidence/mair-anpu/Check.csproj) odwołuje się do badanej biblioteki MAIR.

## Podejrzane miejsce i nasza poprawka

`Compiler.cs`: `ProbeAllArrangements` / `FindChangedLinks`. W MAIR lista już znanych połączeń służy też do odrzucania nowych połączeń znalezionych podczas sprawdzania układów tłoków. To może odrzucić ścieżkę, która otwiera się dopiero przy kilku przesunięciach naraz.

Nasz Redpiler odkrywa takie ścieżki w grafie połączeń, także bez wcześniejszego „pierwszego” połączenia. Mamy małe testy dwóch blokad oraz test wskazanej pochodni w ANPU. Podczas działania skompilowanego układu nie uruchamiamy w tym celu zwykłej symulacji.

Przy uruchamianiu zapisanego projektu poza dawnym katalogiem trzeba wskazać właściwą bibliotekę w `HintPath` pliku projektu. Zapisany plik zachowuje oryginalną ścieżkę względną `../mair-crosscheck/bin/MAIR.Core.dll`.

[Wynik MAIR z poprawioną flagą lejka](../evidence/mair-anpu/result-4096-hopper-support.json) · [Brakujące źródło w tabeli](../evidence/mair-anpu/wire-table.json) · [Nasz test grafu](../evidence/anpu-local-graph-census.log) · [Test bez wcześniejszej ścieżki](../evidence/anpu-local-routing.log)
