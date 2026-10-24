# Plan: czas kompilacji, pamięć i bezpieczny stos

Status: pierwsza partia wdrożona i zmierzona; cel 10 s nadal nieosiągnięty. Data: 2026-10-24.
Baza analizy: `2f5274a7bbb977290ce36cc89d625e1fe843249e` i aktualny worktree.

## Cel i granice

Zmniejszyć koszt kompilacji rzeczywistych dużych układów, przede wszystkim
CPU_BubbleSort i FPU. Cel użytkowy to maksymalnie 10 s od żądania kompilacji do
gotowości do aktywacji na ustalonym sprzęcie i dla zadeklarowanego trybu.
Raportować także sam czas kompilatora, bez ładowania schematu i wykonywania układu.
Cel 10 s jest kryterium pomiaru, a nie obietnicą wyniku przed pomiarami.

Kompilator nie powinien wymagać powiększania stosu z powodu głębokości grafu.
Jawne stosy robocze muszą jednak mieć ograniczony koszt pamięci i respektować
istniejące budżety. Przeniesienie rekurencji na stertę nie usuwa jej kosztu.

Ta partia nie zmienia semantyki układów, poziomów optymalizacji ani przyjmowanych
założeń. Nie obejmuje narzędzi inspekcji ani `--assert-instant`, opisanych w
[osobnym planie](REDPILER_PLAYER_ANALYSIS_PLAN.md).

Nie wolno uzyskać szybszej kompilacji przez obniżenie pokrycia, wcześniejsze
odrzucanie trudnych przypadków, zmniejszenie budżetów lub pominięcie walidacji.

## Co wiadomo z aktualnego kodu

| Obszar | Ustalenie | Znaczenie |
| --- | --- | --- |
| Powiększony stos | Historyczny raport diagnostycznego rozwijania FPU podaje `RUST_MIN_STACK=33554432`. Nie znaleziono aktualnego nadpisania stosu kompilatora. | Najpierw odtworzyć aktualny problem; nie uznawać starego raportu za świeży pomiar. |
| `Equations::expand` | Rekurencyjny DFS po surowych wyrażeniach; klonowanie `LayerExpr` kopiuje także tablice operandów. | Konkretny cel usunięcia zależności stosu od głębokości DAG-u i zbędnych kopii. |
| Rozwijanie źródeł | Główna ścieżka z `defer_sources` już używa iteracyjnego worklistu. Specjalne rozwijanie repeaterów pozostaje rekurencyjne, z limitem 128. | Nie przepisywać całego rozwiązania źródeł ponownie. Osobno zbadać pozostałe zagnieżdżenia. |
| `SignalTracer` | Zapytania oraz zależności guard/site używają rekurencji i ograniczeń głębokości. | Granica 128 nie dowodzi, że rzeczywisty stos jest mały: poziom może obejmować wiele wywołań. |
| `LayerBuilder::not/combine` | Klonują wyrażenie również wtedy, gdy potrzebują tylko prostego rozpoznania wariantu. | Możliwe usunięcie tymczasowych kopii bez zmiany reprezentacji. |
| Internowanie | Arena i klucze internera posiadają osobne kopie list operandów; budżet uwzględnia te dwie listy. | To cel ewentualnej późniejszej zmiany, wymagający pomiaru, nie automatyczny błąd. |
| Sortowanie źródeł | W siedmiu miejscach `sort_unstable_by_key` formatuje klucze przez `Debug`. | Możliwy nadmiar formatowania i alokacji podczas porównań. |
| Walidacja warstw | Normalny runner przygotowuje wynik kanoniczny i przekazuje zweryfikowany kontekst do loweringu. | Stary profil wielokrotnego kompilowania warstw nie opisuje tej obecnej ścieżki. |

Główne pliki:

- [logical.rs](../../../crates/core/src/redpiler/analysis/instant/layers/logical.rs)
- [equations.rs](../../../crates/core/src/redpiler/analysis/instant/layers/logical/equations.rs)
- [signals.rs](../../../crates/core/src/redpiler/analysis/instant/layers/signals.rs)
- [LayerBuilder](../../../crates/core/src/redpiler/analysis/instant/layers.rs)
- [walidacja passów](../../../crates/core/src/redpiler/passes/validation.rs)
- [przygotowanie i przekazanie własności](../../../crates/core/src/redpiler/passes/rich.rs)
- [lowering](../../../crates/core/src/redpiler/program_graph/lowering.rs)

## Zasady implementacji

- Małe, osobno mierzone zmiany. Nie łączyć naprawy stosu z nowym formatem grafu.
- Nazwane etapy i krótkie funkcje; komentarze wyjaśniają wymagania semantyczne.
- Brak nowego frameworka profilowania, zależności i domyślnie aktywnych śladów.
- Zachować kolejność odwiedzania, deterministyczne ID, internowanie, pierwszeństwo
  błędów oraz kolejność callbacków i efektów.
- Zachować rozróżnienie: unsupported, budget exceeded, cancelled, invalid graph.
- Memoizacja jest ważna wyłącznie dla tego samego grafu, areny i kontekstu
  podstawień. Nie usuwać `expanded.clear()` pomiędzy równaniami bez osobnego
  dowodu zgodności kontekstów.
- Nie zwiększać produkcyjnego stosu ani nie dodawać `stacker` jako rozwiązania.
- Nie uruchamiać kilku ciężkich kompilacji równolegle podczas pomiarów.

## Etap 01 — odtworzenie problemu i baza pomiarowa

### Zadania

1. Zapisać revision, diff, kompilator Rust, profil Cargo, sprzęt, opcje Redpilera,
   mnożnik budżetu, hashe fixture i stan wejściowy.
2. Zbudować bazowy executable raz. Budowanie Rust nie jest mierzoną kompilacją
   obwodu. Nie porównywać różnych buildów z przypadkowo różnymi flagami.
3. Zmierzyć przygotowanie, passy wraz z wymaganą analizą/walidacją, lowering,
   konstrukcję backendu i całkowity czas. W przepływie serwera oddzielnie
   uwzględnić snapshot i oczekiwanie na aktywację.
4. Wykorzystać istniejące statystyki i zegary wokół etapów. Zagnieżdżonych czasów
   inclusive nie sumować jako niezależnych kosztów.
5. Zmierzyć pamięć procesu: na Windows szczyt `PeakWorkingSet64` oraz próbki
   `PrivateMemorySize64`, z częstotliwością podaną w raporcie. Próbkowane maksimum
   nie jest gwarantowanym szczytem. Zachowane tabele nie są pomiarem RSS.
6. Uruchamiać próby w osobnych procesach. Szczyt pamięci całego procesu obejmuje
   też loader i weryfikację; podać ich zakres i nie przypisywać go automatycznie
   kompilatorowi. Raportować próbki z przedziału kompilacji osobno.
7. Odtworzyć diagnostykę FPU bez `RUST_MIN_STACK`, a potem stworzyć mały przypadek
   wyizolowujący głębokie wyrażenia lub zapytania.

Przepełnienie stosu może przerwać proces. Przypadek odtwarzający wykonywać w
procesie potomnym, żeby nie zabić całego zestawu testów. Jawnie mały stos jest
narzędziem testowym; jego rozmiar zapisać, a nie nazywać go domyślnym.

Istniejące punkty wejścia:

- [CPU benchmark](../../../crates/core/benches/cpu_bubblesort.rs) zapisuje
  `graph_compile_seconds` osobno od tickowania.
- [FPU extraction diagnostic](../../../crates/core/src/redpiler/analysis/instant/layers/logical/fpu.rs),
  test `report_fpu_logical_output_equations`, jest kosztowną diagnostyką, nie
  samodzielnym produkcyjnym benchmarkiem.
- [FPU kernel benchmark](../../../crates/core/src/redpiler/analysis/instant/layers/logical/fpu_kernel_bench.rs)
  mierzy także extraction, ale jego throughput i feedback nie dowodzą pełnej
  zgodności czasowej FPU. Nie uruchamiać całej części runtime, jeśli potrzebna
  jest wyłącznie kompilacja.

Przed użyciem filtra testów sprawdzić discovery. Zero dopasowanych testów nie
jest pozytywnym wynikiem.

**Bramka przeglądu:** aktualny profil, pomiary pamięci oraz odtworzony przypadek
stosu albo jednoznaczne stwierdzenie, że starego błędu nie odtworzono. Nie podawać
historycznego profilu jako aktualnej atrybucji kosztów.

## Etap 02 — iteracyjne `Equations::expand`

### Algorytm

Zastąpić rekurencyjne rozwijanie dzieci jawnym DFS/postorder, zachowując aktualną
kolejność od lewej do prawej:

1. Wejście do węzła wykonuje ten sam `visit()` i sprawdzenie memoizacji.
2. Stała i wejście zachowują obecne operacje buildera oraz rozwiązywania źródła.
3. Ramka operatora przechowuje ID rodzica i indeks następnego dziecka. Dzieci
   pobierane są pojedynczo z areny; nie klonować całej tablicy operandów.
4. Po rozwinięciu dzieci zebrać ich wyniki w oryginalnej kolejności i wywołać
   istniejące `not` lub `combine`.
5. Do memoizacji wpisać wyłącznie zakończony wynik.

Kursor dziecka ogranicza liczbę ramek do głębokości przejścia zamiast wrzucania
całego szerokiego poddrzewa na stos naraz. Nie tworzyć `Vec` dla każdego dziecka.
Pamięć ramek musi być rozliczona albo ograniczona istniejącym budżetem z
udokumentowaną górną granicą.

**Istotne ponowne wejście:** `expand(Input) -> source -> query -> expand` może
rozbudować surową arenę. Nie trzymać referencji do jej elementów przez mutujące
wywołanie. Lokalne ramki muszą przetrwać to wywołanie bez nadpisania przez
współdzielony scratch. Usunięcie zwykłego DFS nie usuwa tej pośredniej rekurencji.

Nie zmieniać mapy memoizacji na gęsty wektor w tej samej poprawce. Taka zmiana
wymaga sprawdzenia gęstości używanych ID, kosztu czyszczenia i kontekstów cache.

### Testy

- Głęboki, poprawny DAG z naprzemiennymi operatorami, którego builder wcześniej
  nie uprości do stałej; wykonanie na zapisanym małym stosie.
- Szerokie operatory i wielokrotnie współdzielone podwyrażenia.
- To samo wyrażenie odwiedzane z cache; niezmienione korzenie, wejścia i ID.
- Ponowne wejście przez źródło, które dopisuje wyrażenia do surowej areny.
- Niepoprawne referencje, anulowanie i wyczerpanie budżetu.
- Cykle źródeł nadal obsługiwane przez istniejący mechanizm, a nie ignorowane.

**Bramka przeglądu:** zgodność małych przypadków z wersją bazową, test głębokiego
DAG-u i brak zmiany przyjętych/odrzuconych fixture. Dopiero wtedy pomiar realnych
układów. Nie deklarować całkowitego rozwiązania problemów stosu po jednym DFS.

## Etap 03 — usunięcie zbędnych kopii i formatowania

Wprowadzić osobno, tak aby było wiadomo, która zmiana daje efekt.

1. W `LayerBuilder::not` i `combine` rozpoznawać wariant przez pożyczone
   wyrażenie. Kopiować tylko małe wartości potrzebne po zakończeniu pożyczenia.
   Zachować obecne uproszczenia i kolejność rozliczania pracy.
2. Sprawdzić pozostałe `expression(...).clone()` w tej ścieżce; usuwać wyłącznie
   kopie bez wymaganego własnościowego lub semantycznego zastosowania.
3. Dla sortowania źródeł porównać `sort_by_cached_key` z dotychczasowym
   formatowaniem klucza przy porównaniach. Zachować dokładnie obecny porządek
   tekstowy, również dla krawędzi z kluczem złożonym.
4. Potwierdzić jednoznaczność kluczy albo zachować rozstrzyganie remisów.
   Zmiana porządku na kolejność pól enumu jest osobnym refaktorem.

Cached sorting zmniejsza wielokrotne formatowanie, ale przetrzymuje wszystkie
klucze jednocześnie. Może zwiększyć pamięć szczytową. Zachować je tylko po
ocenie czasu i pamięci, szczególnie dla dużych list.

Testy: stałe, negacja negacji, sprzeczność, tautologia, puste operatory,
współdzielone podwyrażenia, deterministyczny wynik niezależny od kolejności
wstawiania do map i niezmienione obsługiwanie limitów.

**Bramka przeglądu:** zgodność artefaktów i osobne pomiary dla kopii oraz kluczy.
Nie uznawać zmniejszenia liczby alokacji za dowód krótszej kompilacji.

## Etap 04 — pozostałe ryzyko stosu

Po etapie 02 sprawdzić rzeczywiste zagnieżdżenia:

- `SignalTracer::query -> contribution -> source -> query`;
- `guard -> site_test -> guard`, w tym warunkowe kontakty;
- rozwijanie inline repeaterów w trybie deferred;
- niedeferredowe rozwijanie źródeł używane przez diagnostykę.

Przepisywać tylko ścieżki, dla których test lub pomiar pokazuje ryzyko albo
istotny koszt. Jeżeli potrzebna jest iteracja, ramki powinny odzwierciedlać
istniejące etapy: zapytanie, wkład, guard, wybór kontaktu i dokończenie operatora.
Nie budować ogólnej maszyny wykonującej dowolne operacje kompilatora.

Zachować w szczególności:

- pierwszą pasującą wersję kontaktu i kolejność sprawdzania wariantów;
- rozdzielenie progów, kanałów power i trybu `include_dust`;
- wczesny powrót po fałszywym guardzie, zanim zostanie odczytane źródło;
- rozróżnienie dodatniej straty na cyklu i cyklu przewodzenia bez straty;
- znaki `pending` oddzielone od ukończonych wyników;
- eksportowanie tylko ukończonej memoizacji po błędzie;
- istniejące limity głębokości i zachowanie fallbacku w tej partii.

Usunięcie rekurencji nie upoważnia do automatycznego zwiększenia tych limitów.
Są również ograniczeniami pracy/admisji; ich zmiana wymaga osobnego przeglądu.

Testy oprzeć o [istniejące testy SignalTracer](../../../crates/core/src/redpiler/analysis/instant/layers/signals/tests.rs):
cykle, wszystkie progi sygnału, tłumienie, pierwszeństwo kontaktów, anulowanie,
nieudane zapytanie i ponowne użycie poprawnej memoizacji. Dodać głębokie guardy,
łańcuchy repeaterów oraz przeplot tych zależności na małym stosie.

**Bramka przeglądu:** brak nowych akceptacji/odrzuceń oraz poprawne błędy zamiast
przepełnienia stosu dla przypadków granicznych.

## Etap 05 — pamięć i czas życia danych na podstawie pomiaru

Na podstawie nowego profilu wskazać dominujące jednocześnie żywe struktury:
surową arenę, wynikową arenę, internery, indeksy, kontekst analizy, dane
walidacyjne oraz lowering. Liczyć pojemności i listy operandów, nie tylko liczbę
wyrażeń.

Kolejność rozważanych zmian:

1. Zakończyć niepotrzebne czasy życia struktur przez przekazanie własności lub
   zawężenie zakresu. Zwolnić dopiero po ostatnim użytkowniku i walidacji.
2. Usunąć udowodnione lokalne duplikaty; współdzielić istniejący niezmienny
   kontekst wyłącznie wtedy, gdy dotyczy tego samego grafu i konfiguracji.
3. Rozważyć gęste memo tylko tam, gdzie używane ID rzeczywiście są gęste.
4. Dopiero jeśli operandowe kopie internera dominują, zaplanować pojedynczego
   właściciela list i bezpieczny lookup uwzględniający kolizje hashy. Nie zastępować
   równości semantycznej porównywaniem samego hasha.

Nie dodawać `shrink_to_fit` do każdej kolekcji ani globalnych cache. Takie zmiany
mogą zwiększyć liczbę alokacji lub przetrzymywać pamięć między kompilacjami.
Nie wykonywać całego etapu 05, jeśli wcześniejsze małe zmiany spełnią cel.

**Bramka przeglądu:** niższy zmierzony szczyt albo krótszy czas z zaakceptowanym,
jawnie opisanym kosztem pamięci. Estymata tabel jest tylko dodatkową informacją.

## Etap 06 — weryfikacja rzeczywistych układów i raport

### Macierz

| Wejście | Co sprawdzić |
| --- | --- |
| ADDER1BIT | Mały punkt odniesienia: pełne znane przypadki, artefakty i deterministyczność. |
| ADDER11BIT | Skalowanie, identyczne porty/korzenie, poprawne wyniki. |
| Divider | Głębsze zależności, pokrycie i niezmienione decline/fallback. |
| COUNTER i PC COUNTER | Pamięć BUD, niezależne sterowania, reset i pending work. |
| FPU | Czas/pamięć i bezpieczne ukończenie analizy; dopuszczalne dotychczasowe luki w dowodzie muszą pozostać jawne. |
| CPU_BubbleSort | Publiczna kompilacja, czas całkowity, pamięć i krótki sprawdzony przebieg RAM. |

Porównać O0 oraz wybrane używane poziomy, w tym O6 z poprawnie zadeklarowanym
protokołem wejść. Używać tych samych mnożników budżetu, ticków i fixture.
Nie zakładać, że FPU jest w całości instantowe, aby wymusić sukces kompilatora.

Pomiary bazowe i nowe wykonywać naprzemiennie, po pięć świeżych procesów na
główny wariant; jeżeli koszt jest zbyt duży, najpierw jedna para do wstępnej
oceny, bez mocnego wniosku o speedupie. Zapisać wszystkie próbki, mediany i
zakresy. Profil próbkujący uruchomić osobno, dla jednego wariantu i z widokiem
ograniczonym do kompilacji; jego czasu nie używać jako benchmarku.

Po testach jednostkowych uruchomić odpowiednie testy passów, walidacji, loweringu
i Graph/interpreter. Sprawdzić outputs, pamięć oraz pending work/handoff, nie
tylko liczbę wyrażeń. Pełny sort CPU zostawić na końcową bramkę, gdy krótkie
przebiegi i kompilacja przechodzą; nie powtarzać go po każdym małym refaktorze.

Raport końcowy musi podać:

- osobny wynik każdej zmiany i wynik całej partii;
- bazową i nową wersję źródeł oraz dokładne komendy;
- czas kompilatora, czas użytkowy i pamięć z definicją pomiaru;
- zachowane pokrycie, wielkość artefaktów i rozkład fallbacków;
- sprawdzone rozmiary stosu oraz pozostałe ścieżki rekurencyjne;
- osiągnięcie albo brak osiągnięcia celu 10 s;
- pominięte testy, niestabilność wyników i pozostałe ograniczenia.

## Podział pracy i warunki zakończenia

Luna 6 może wykonać edycje i większe serie weryfikacji. Osobne, rozłączne zadania:
iteracyjne `expand`, lokalne kopie/sortowanie oraz pomiary/pozostała rekurencja.
Nie edytować równolegle tych samych plików i nie mierzyć wydajności podczas
innych buildów. Po każdym etapie przegląd zmian i wyników przed kolejnym.

Pierwsza partia powinna objąć etapy 01–03. Etapy 04–05 zależą od uzyskanych
wyników, a nie od chęci przepisania całego kompilatora.

Zakończenie wymaga testów bez produkcyjnego powiększania stosu, zachowania
semantyki i admisji oraz uczciwego pomiaru czasu i pamięci. Jeżeli 10 s nadal nie
jest osiągnięte, wskazać aktualny dominujący koszt i osobno zaplanować zmianę
algorytmiczną. Nie przedstawiać samego refaktoru jako rozwiązanej wydajności.

## Wynik pierwszej partii — 2026-10-24

Implementację i większe serie pomiarów wykonały trzy agenty Luna 6; zmiany
zostały przejrzane przez agenta głównego. Porównane wersje kompilatora:

- baza: `444c5a07f1a204754dcf990ded75c765dbad8871`;
- wynik: `d5fe331a0cdd9ab1db428751cd235e9ec9d0e778`.

Wersja wynikowa została w trakcie pracy zacommitowana przez inny proces.
Późniejsze równoległe zmiany narzędzi diagnostycznych nie są częścią tego
porównania. Pomiary i testy wykonano przypiętymi kopiami plików wykonywalnych,
więc późniejsze edycje worktree nie zmieniają zakresu tych wyników.

### Co zmieniono

1. `Equations::expand` używa jawnych ramek DFS z kursorem dziecka. Zachowuje
   kolejność, memoizację, rozliczanie pracy i obsługę ponownego wejścia, bez
   klonowania listy operandów na potrzeby przejścia.
2. `LayerBuilder::not/combine` rozpoznaje warianty przez pożyczenie wyrażeń.
3. Sortowanie źródeł i krawędzi buforuje klucz `Debug` jednokrotnie na element,
   zachowując dotychczasowy porządek tekstowy.
4. Walidacja programu logicznego nie konstruuje już tymczasowego ewaluatora.
   Konstruktor i walidator współdzielą te same kontrole referencji.
5. Wybór kandydata lampy pomija skan całego katalogu portów, gdy zbiór
   `lamp_occurrences` jest pusty. Wcześniej ten bezskuteczny skan powtarzano dla
   wielu komórek. To największe potwierdzone źródło poprawy czasu tej partii.

Wstępny eksperyment z wcześniejszym sprawdzaniem efektów lampy nie dał
przekonującej poprawy i został usunięty. Nie dodano nowej reprezentacji grafu,
cache globalnego, zależności ani produkcyjnego powiększania stosu.

### Pomiar CPU_BubbleSort

Ryzen 9 5950X, 80 GiB RAM, Windows/MSVC, Rust 1.98.1, profil release z fat LTO.
O6, stabilne wejścia, mnożnik budżetu 8. Pięć par świeżych procesów,
naprzemienna kolejność wersji. Ładowanie, aktywacja, 100 ticków wykonania,
flush, odczyty RAM i reset nie należą do mierzonego czasu kompilatora.

| Para | Baza [s] | Wynik [s] |
| --- | ---: | ---: |
| 1 | 45,409293 | 18,652196 |
| 2 | 45,691492 | 19,339264 |
| 3 | 41,692965 | 18,952340 |
| 4 | 42,311949 | 19,182929 |
| 5 | 41,209886 | 18,605952 |
| Mediana | **42,311949** | **18,952340** |

Stosunek median to około **2,23×**, czyli czas mniejszy o około **55,2%**.
Mediana redukcji liczonej osobno w każdej parze wynosi **54,85%**.
Cel 10 s nie został osiągnięty. To czas kompilatora, a nie pełne opóźnienie
żądania serwerowego wraz ze snapshotem i oczekiwaniem na aktywację.

Mediana `compile-settled-circuit` po zmianie wynosi 3,307 s: budowa 1,229 s,
walidacja 2,070 s. Czasy są inclusive; nie sumować etapów zawierających te same
wywołania. Dalsza praca powinna objąć koszt ponownej walidacji i analiz
zależności, z zachowaniem ich kontroli semantycznych.

Szczyt working set całego procesu pozostaje podobny: baza 915,1–916,6 MiB,
wynik 915,9–917,4 MiB. Obejmuje loader i sprawdzanie wykonania; to nie jest
wyizolowany szczyt kompilatora. Próbki prywatnej pamięci zbierano co 50 ms.
Nie wykazano globalnej redukcji pamięci. Cached sorting przetrzymuje klucze
jednocześnie; usunięte lokalne alokacje nie wystarczają do uznania celu RAM
za osiągnięty.

W każdej parze zgadzały się hashe wejścia i RAM, checkpointy aktywnego
wykonania/resetu/32 ticków kontynuacji oraz wszystkie liczniki pracy grafu.
Schemat na dysku nie był modyfikowany: blokujący znak usunięto identycznie
w pamięci z wejść obu wersji. SHA-256 fixture:
`803dbe5d6751b152bea9b134d6c8571cfafa3e840e372f82e41a535b658f1611`.

Pełny przebieg wynikowej wersji obejmował **10 036 ticków**, porównanie RAM
w każdym ticku z zamrożonym śladem interpretera i wynik **0..15**. Samo tickowanie
zajęło 22,799 s; był to pojedynczy test poprawności, nie sparowany pomiar
przyspieszenia wykonania. Wykonano też 32 ticki po handoff i zapisano stan;
ten pojedynczy przebieg nie dowodzi niezależnie zgodności całej kontynuacji.

### Stos i ograniczenia FPU

Test 20 000-poziomowego DAG-u przechodzi z żądanym stosem roboczym 128 KiB.
Osobny izolowany test potwierdza przepełnienie tego stosu przez starą
rekurencyjną implementację-oracle. Porównanie małych DAG-ów obejmuje kolejność
ID, areny, memoizację, budżety, błędy i ponowne wejście podczas dopisywania
wyrażeń.

Nie odtworzono aktualnego przepełnienia stosu FPU. Diagnostyka bazowa i wynikowa
kończą się tym samym istniejącym błędem dla `ResourceId(6079)` /
`ContactId(21085)`: `directional corridor output depends on dot/cross state`.
Nie jest to udana kompilacja FPU ani dowód, że wskazany układ jest nieinstantowy.
Pozostają rekurencja zapytań/guardów `SignalTracer`, inline repeaterów i pośrednie
ponowne wejścia przez źródła. Nie ogłaszamy całego problemu stosu rozwiązanym.

### Sprawdzenie i odtworzenie

Budowa przypiętych executable:

```powershell
cargo test -p mchprs_core --lib --bench cpu_bubblesort --release --locked --no-run
```

Krótki pomiar wykonywano osobno dla bazowej i wynikowej kopii harnessu:

```powershell
target/compiler-resources/final-cpu.exe --ticks 100 --graph-tier 6 --stable-instant-inputs --iterations 1 --output target/compiler-resources/sample.json
```

Pełny sort używa tych samych argumentów bez `--ticks 100`. Raporty źródłowe,
kolejność par, próbki pamięci i czasy etapów są w
[paired-results.json](../../../target/compiler-resources/paired-results.json);
jednorazowy wrapper `run-sample.ps1` w tym samym katalogu zapisuje zakres
pomiaru pamięci. Są to lokalne artefakty w ignorowanym `target`, nie trwały
framework profilowania.

Przeszły `cargo check -p mchprs_core --lib --tests --locked`,
`cargo check -p mroww --locked`, `cargo fmt -p mchprs_core -- --check`
oraz `git diff --check` dla zmienionej wersji kompilatora. Testy obejmują
iteracyjne rozwijanie, builder, sortowanie, walidację i skan kandydatów.
Końcowy zestaw release Redpilera: **684 zaliczone, 0 błędów, 87 ignorowanych**
(93,86 s; przypięty executable, po normalizacji końców linii snapshotów).
Początkowy błąd snapshotu JSON występował również w przypiętej bazie:
worktree miało CRLF zamiast LF. Wąska reguła `.gitattributes` ustala LF dla
sześciu snapshotów ADDER1BIT, bez zmiany wartości JSON; dokładny test przechodzi
po normalizacji w obu wersjach.

Nie wykonano pełnego workspace, całego zestawu runtime debug ani fuzzingu.
Etapy 04–05 pozostają otwarte; do następnej partii potrzebny jest pomiar
pamięci w granicach samej kompilacji i analiza czasu życia największych danych.
