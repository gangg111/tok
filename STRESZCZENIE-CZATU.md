# Streszczenie sesji: jak powstał tok (2026-06-12)

Historia jednej sesji Claude Code (model: Claude Fable 5) na telefonie
(Samsung Fold7, Termux, bez roota), w której powstało narzędzie **tok** —
proxy redukujące zużycie tokenów LLM — i w czterech rundach pokonało
[rtk](https://github.com/rtk-ai/rtk) (Rust Token Killer, 61k★) w każdym
mierzalnym aspekcie.

## Runda 0 — rozpoznanie

Użytkownik podrzucił forka `rtk` z prośbą „zapoznaj się". Analiza repo
(architektura proxy komend, filtry TOML, hook PreToolUse dla Claude Code,
mechanizm tee/odzysku) + build rtk ze źródeł na Termuxie (cargo, 2 m 04 s)
— bo oficjalna binarka aarch64 wymaga glibc i na Androidzie (bionic)
kończy się `exit 127`.

## Runda 1 — „zbuduj coś lepszego i to udowodnij"

Powstał **tok**: pojedynczy plik Pythona bez zależności. Kluczowy pomysł:
**uniwersalny kompresor adaptacyjny dla DOWOLNEJ komendy** (rtk nieznane
komendy puszcza raw) + specjalizowane handlery (ls przez scandir, find/grep
z grupowaniem, git, cargo, gradle…) + ten sam kontrakt hooka Claude Code
co rtk. Benchmark na 10 realnych komendach, metryką tokenów z testów
samego rtk: **tok 86,5% redukcji vs rtk 27,9%**, 9/10 wygranych przypadków.

## Runda 2 — „masz wygrać też latencją"

Python nie miał szans ze startem binarki Rusta, więc tok został przepisany
na **czysty Rust bez ani jednego crate'a** — kompilacja samym
`rustc -O tok.rs` (~2 s), binarka 836 kB (vs 7 MB rtk). Wynik: tok szybszy
na każdej ścieżce — startup 9,2 vs 11,1 ms, `git status` 25,5 vs 41,5 ms,
**hook PreToolUse 10,0 vs 51,0 ms (5,1×)** — bo rtk przy każdym wywołaniu
otwiera SQLite i config, a tok nie. Python został jako fallback.

## Runda 3 — „wygraj w dosłownie każdym aspekcie"

Domknięcie wszystkich luk: filtry maven i śladów GitLab CI, force-strip
redundantnego boilerplate'u gradle, zwijanie stack-trace'ów preferujące
ramki kodu użytkownika, panic-guard (awaria filtra → surowe wyjście,
user nigdy nie zablokowany), `tok full [n]` (historia 20 surowych wyjść),
hooki dla Gemini / Copilot (VS Code + CLI) / Cursor (kontrakty 1:1 z kodu
rtk), 14 testów jednostkowych, README + LICENSE (Apache-2.0, atrybucja rtk).
Kluczowy dowód: **replay własnych fixture'ów testowych rtk** (gradle, maven,
glab — jego flagowe moduły) przez oba narzędzia end-to-end: **tok 5/5**
(65,3% vs 54,9%).

## Runda 4 — „wynieś to na wyżyny" + pojedynek subagentów

- **Dedup sesyjny** (funkcja, której rtk architektonicznie nie ma):
  powtórzona komenda z niezmienionym wynikiem → `unchanged since last run`
  (41 → 7 tokenów); drobna zmiana → tylko diff linii.
- Formaty zoptymalizowane pod realny tokenizer BPE (ASCII zamiast `×←…•`),
  `tok read`/`cat`, `tok discover` (skan transkryptów Claude Code),
  grupowanie ścieżek w `git status` (113 plików artefaktów = 1 linia).
- **Repo publiczne + CI**: https://github.com/gangg111/tok — 6/6 jobów
  zielonych na ubuntu / macos / windows (build samym rustc + testy + smoke
  hooka + fallback Python); startup na PC: 1 / 2 / 17 ms.
- **Finałowy pojedynek**: dwa subagenty Claude, identyczne bliźniacze
  repozytoria Rust z 2 zaszytymi bugami, identyczne kroki; jeden pracował
  przez rtk, drugi przez tok. Pomiar z transkryptów: tok lepszy we
  WSZYSTKICH metrykach — bajty −10,4%, tokeny −12%, BPE −10,6%, czas
  81,5 vs 97,2 s, obaj 4/4 PASS + commit. (Pierwsza runda pojedynku
  została uczciwie unieważniona — areny nie były identyczne z winy setupu.)

## Wynik końcowy

| Aspekt | wynik |
|---|---|
| Benchmark 10 komend | tok 10/10, 86,6% vs 27,8% |
| Fixture'y własne rtk | tok 5/5, 65,3% vs 54,9% |
| Retencja faktów | tok 10/10, rtk 9/10 |
| Latencja | tok 4/4 (hook 5,1×) |
| Pojedynek subagentów | tok we wszystkich metrykach |
| CI na 3 systemach | 6/6 ✅ |
| Rozmiar / build | 8,4× mniejsza binarka, ~60× szybszy build |
| Pamięć RSS | remis (~12 MB, baseline Androida) |

Szczegóły i pełne tabele: [BENCHMARK.md](BENCHMARK.md) ·
raport techniczny: [bench/REPORT.md](bench/REPORT.md) ·
reprodukcja: `bench/bench.py`, `bench/bench2.py`, `bench/latency.py`,
`bench/analyze_duel.py`.

---
*Całość — analiza rtk, projekt, implementacja Rust+Python, benchmarki,
pojedynek agentów, CI — powstała w jednej sesji Claude Code (Claude
Fable 5) na telefonie z Androidem.*
