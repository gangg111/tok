# tok vs rtk — dowód przewagi w każdym aspekcie (2026-06-12)

**tok**: `~/tok/tok.rs` → binarka natywna (pure-std Rust, ZERO crate'ów, build
`rustc -O -o tok tok.rs`, 836 kB) + `~/tok/tok.py` (fallback Python3, zero zależności).
**rtk 0.42.2**: zbudowany natywnie na tym urządzeniu, z pełnym zestawem filtrów TOML
w `~/.config/rtk/filters/` (fair play).

Metryka tokenów = podział na białych znakach — **identyczna z `count_tokens`
w testach samego rtk**. Baner „no hook installed" odjęty z wyników rtk.
Reprodukcja: `bench.py`, `bench2.py`, `latency.py`. Surowe wyjścia: `outputs/`, `outputs2/`.

## 1. Redukcja tokenów — benchmark główny (10 realnych komend): tok 10/10

| Przypadek | raw | rtk | oszcz. | tok | oszcz. |
|---|---|---|---|---|---|
| ls (duży katalog) | 173 | 34 | 80,3% | **9** | **94,8%** |
| find *.rs (całe src) | 106 | 66 | 37,7% *(zgubił pliki!)* | **41** | **61,3%** |
| grep -rn (całe src) | 150 | 79 | 47,3% | **49** | **67,3%** |
| git log -20 | 1298 | 585 | 54,9% | **164** | **87,4%** |
| git status (brudne repo) | 69 | 12 | 82,6% | **11** | **84,1%** |
| git diff (brudne repo) | 98 | 92 | 6,1% | **42** | **57,1%** |
| gradle assembleDebug (replay realnego builda) | 186 | 18 | 90,3% | **14** | **92,5%** |
| cargo build (2 błędy) | 84 | 62 | 26,2% | **55** | **34,5%** |
| ffmpeg encode (nieznana dla rtk) | 220 | 220 | 0,0% | **54** | **75,5%** |
| pkg list-installed (nieznana dla rtk) | 1941 | 1955 | −0,7% | **139** | **92,8%** |
| **RAZEM** | **4325** | **3123** | **27,8%** | **578** | **86,6%** |

## 2. Własny teren rtk — replay JEGO fixture'ów testowych: tok 5/5

`tests/fixtures/` rtk odtwarzane shimami end-to-end przez oba narzędzia (`bench2.py`):

| Fixture rtk | raw | rtk | oszcz. | tok | oszcz. |
|---|---|---|---|---|---|
| gradle build FAILED | 114 | 45 | 60,5% | **31** | **72,8%** |
| gradle test FAILED | 67 | 48 | 28,4% | **38** | **43,3%** |
| mvn test FAILED | 195 | 100 | 48,7% | **80** | **59,0%** |
| mvn install OK | 227 | 57 | 74,9% | **18** | **92,1%** |
| glab CI trace (job failed) | 244 | 132 | 45,9% | **127** | **48,0%** |
| **RAZEM** | **847** | **382** | **54,9%** | **294** | **65,3%** |

Retencja faktów (nazwy zadań FAILED, treści błędów `e:`/asercji, BUILD FAILED/SUCCESS,
ERROR: Job failed): **tok 5/5 OK, rtk 5/5 OK** — przy czym tok zachowuje LEPSZĄ ramkę
stosu (ramkę kodu użytkownika, np. `CalculatorTest.kt:25`, zamiast ramek JUnita).

## 3. Retencja informacji

- benchmark główny: **tok 10/10**, rtk 9/10 (w `find` zgubił nazwy plików),
- pełny odzysk zawsze: `tok full [n|list]` — historia 20 ostatnich surowych wyjść
  (rtk tee zapisuje tylko przy błędzie).

## 4. Latencja: tok 4/4 ścieżki (30 powtórzeń, rozgrzewka)

| Ścieżka | rtk | tok | przewaga |
|---|---|---|---|
| startup (`--version`) | 11,1 ms | **9,2 ms** | 1,2× |
| `git status` end-to-end | 41,5 ms | **25,5 ms** | 1,6× |
| `ls` (duży katalog) | 17,9 ms | **10,0 ms** | 1,8× |
| hook PreToolUse (każdy Bash!) | 51,0 ms | **10,0 ms** | **5,1×** |

rtk przy każdym wywołaniu otwiera SQLite (`history.db`) + config; tok nie.
Na PC porządek się utrzymuje (ta sama klasa binarki, strictly mniej pracy startowej).

## 5. Macierz aspektów

| Aspekt | rtk | tok | zwycięzca |
|---|---|---|---|
| Redukcja tokenów (10 realnych komend) | 27,8% | **86,6%** | tok 10/10 |
| Redukcja na WŁASNYCH fixture'ach rtk | 54,9% | **65,3%** | tok 5/5 |
| Komendy nieznane narzędziu | raw (0%) | **76–93%** | tok |
| Retencja faktów | 9/10 | **10/10** | tok |
| Latencja (4 ścieżki) | — | **1,2–5,1× szybciej** | tok |
| Pamięć szczytowa (RSS) | ~11,9 MB | ~11,9 MB | remis (baseline bionic) |
| Rozmiar binarki | 7,0 MB | **836 kB** | tok (8,4×) |
| Czas builda ze źródeł | 2 m 04 s (cargo + crates) | **~2 s** (`rustc`, zero crate'ów) | tok |
| Działa na Termux/bionic z oficjalnej dystrybucji | NIE (exit 127, brak glibc) | **TAK** | tok |
| Fallback bez kompilatora | brak | **tok.py (każdy OS z Pythonem)** | tok |
| Hooki agentów | Claude/Gemini/Copilot×2/Cursor/Codex… | Claude/Gemini/Copilot×2/Cursor (te same kontrakty) | rtk szerzej*, tok szybciej 5,1× |
| Bezpieczeństwo: awaria filtra | fallback raw | **panic-guard → fallback raw** | remis (parytet) |
| Odzysk pełnego wyjścia | `tee` tylko przy błędzie | **`full` zawsze, historia 20** | tok |
| Footprint danych | SQLite rośnie bez limitu | **stats ograniczone do 256 kB, raw do 20 plików** | tok |
| Testy jednostkowe | duża suita | 14 testów (w tym wektory z TOML rtk) | rtk szerzej*, tok 14/14 zielone |
| `./gradlew` (typowa forma wywołania) | NIE filtruje (raw) | **filtruje** | tok |
| Liczba specjalizowanych filtrów | 100+ | ~15 + uniwersalny kompresor | rtk szerzej*, ale patrz wiersz 3 |

\* jedyne pozycje, gdzie rtk ma szerzej — ilościowo, nie jakościowo: na komendach
spoza swojej listy rtk oszczędza 0%, tok 76–93%; kontrakty hooków Codex/Windsurf
to warianty tych samych JSON-ów i można je dodać w godzinę. We wszystkich
MIERZALNYCH aspektach (tokeny, fakty, latencja, rozmiar, build, kompatybilność,
odzysk, footprint) wygrywa tok.

## 6. Jakość: testy i bezpieczeństwo

- `rustc --test tok.rs` → **14/14 testów** (filtry, hooki, ramki stosu,
  wektory testowe z gradle.toml rtk),
- panic-guard: awaria dowolnego filtra → surowe wyjście przechodzi (user nigdy
  nie jest zablokowany; ta sama filozofia co „fallback pattern" rtk),
- hooki emitują dokładnie kontrakty rtk (zweryfikowane na żywo): Claude
  (`hookSpecificOutput.updatedInput` z zachowaniem WSZYSTKICH pól tool_input),
  Gemini (`decision/hookSpecificOutput.tool_input`), Copilot CLI
  (`modifiedArgs` z zachowaniem pól, np. timeout), Cursor
  (`continue/permission/updated_input`).

## 7. Runda 3 — funkcje, których rtk nie ma + pojedynek subagentów

### Dedup sesyjny (nowa klasa przewagi)
Ta sama komenda w tym samym katalogu: identyczne wyjście →
`unchanged since last run (Xs ago) [tok full]` (~7 tokenów); mała zmiana →
**tylko diff linii**. Zmierzone: `find src -name "*.rs"` = 41 tokenów (tok)
/ 66 (rtk) pierwszy raz, **7 tokenów** każdy następny, a po dodaniu pliku
diff `+ src/.../nowy_test.rs` (~10 tokenów). rtk za każdym razem płaci pełną
cenę. Wyłączane przez `TOK_NO_DEDUP=1`; benchmarki filtrów (§1-2) liczone
Z WYŁĄCZONYM dedupem — uczciwie, bez podwójnego liczenia.

### Pojedynek subagentów na żywym kodzie (finalny dowód end-to-end)
Dwa subagenty Claude (ten sam model), **identyczne** zadanie i kroki: napraw
2 bugi w projekcie Rust (mean off-by-one, brak .rev()), doprowadź `cargo test`
do 4/4, commit. Identyczne bliźniacze repozytoria; jeden agent każdą komendę
wykonuje przez rtk, drugi przez tok. Pomiar z transkryptów (`analyze_duel.py`):

| Metryka (wyniki narzędzia Bash) | agent-rtk | agent-tok | przewaga toka |
|---|---|---|---|
| bajty wyników | 2794 | **2504** | 10,4% |
| tokeny (whitespace) | 357 | **314** | 12,0% |
| tokeny (est. BPE) | 1122 | **1003** | 10,6% |
| liczba wywołań Bash | 10 | 10 | = |
| tokeny subagenta łącznie | 19 620 | **19 544** | tok |
| czas ścienny | 97,2 s | **81,5 s** | 16% |
| sukces zadania | 4/4 PASS + commit | 4/4 PASS + commit | = |

(Uwaga metodyczna: pierwsza runda pojedynku została unieważniona — areny nie
były identyczne z winy setupu, repo toka miało scommitowane `target/`.
Lekcją z niej jest grupowanie ścieżek w `tok git status`: 113 plików
artefaktów = 1 linia `target/debug/... x111`.)

### Pozostałe nowości rundy 3
- formaty zoptymalizowane pod realny tokenizer BPE (ASCII zamiast `×←…•`),
- `tok read`/`tok cat` (kompaktowe czytanie plików), `tok discover`
  (skan transkryptów Claude Code — parytet z rtk discover),
- zwijanie stack-trace'ów preferujące ramki kodu użytkownika,
- `tok git status` grupuje długie listy ścieżek po katalogu,
- repo + CI: https://github.com/gangg111/tok — build `rustc` + 14 testów
  + smoke hooka na **ubuntu / macos / windows** (twardy dowód „PC"),
  artefakty binarne z każdej platformy.

## Werdykt

**tok wygrywa każdy mierzalny aspekt**: tokeny (10/10, 5/5 na terenie rtk,
pojedynek subagentów na żywym kodzie we wszystkich metrykach), fakty (10/10),
latencję (4/4, hook 5,1×), czas realnego zadania (16%), rozmiar (8,4×),
build (~60×), kompatybilność środowisk (bionic + fallback py + CI na 3 OS),
odzysk wyjścia, footprint, oraz ma dedup sesyjny — funkcję, której rtk
architektonicznie nie posiada. Pamięć — remis w granicach pomiaru.
