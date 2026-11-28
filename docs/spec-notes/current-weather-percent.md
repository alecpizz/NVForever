# Script weather measurements

This source proposal joins `GetCurrentWeatherPercent`, `IsSnowing`, `IsPleasant`
and `IsCloudy` at the existing sky-to-script report. It adds no weather timer,
selection policy, catalogue entry, native version claim or serialized state.

## Contracts and uncertainty

[documented] The GECK references describe
[GetCurrentWeatherPercent](https://geckwiki.com/index.php/GetCurrentWeatherPercent)
as transition progress in 0..1 and provide the alias `getweatherpct`.
[IsPleasant](https://geckwiki.com/index.php?title=IsPleasant) describes destination
Pleasant classification with intermediate transition values.
[IsSnowing](https://geckwiki.com/index.php?title=IsSnowing) describes a settled
Snow result and explicitly distinguishes Fallout's binary result from fractional
Oblivion behavior. Its separate example appears inconsistent with that settled
description; these sources are not native endpoint or timing measurements.

`IsCloudy` uses the corresponding destination-Cloudy fraction described by the
repository catalogue and corroborated by the GECK Snow/Rain pages' distinction
between binary and fractional weather queries. The linked
[Construction Set reference](https://cs.uesp.net/wiki/IsCloudy) is for Oblivion,
not an FNV execution result. The exact FNV `IsCloudy` page could not be retrieved.
The selected destination-class rule is an explicit portable policy where that
provenance stops; native outgoing/reversal behavior and endpoint timing remain
unmeasured. No outgoing `1-fade` contribution is inferred.

The existing WTHR DATA importer supplies classification bits 1 Pleasant, 2 Cloudy,
4 Rainy and 8 Snow. `SkyState` already owns destination weather, previous weather,
and destination progress. Those are repository source facts. The batch reads
them without changing their producer, fade scheduling, override/release,
fast-travel hold or `keep_weather_once` behavior. Existing `IsRaining` is unchanged.
Unrelated classification bits do not manufacture a known class. Existing JIP
`SetWeatherTraitNumeric` session overrides remain stored and visible to that
trait getter; fog traits `15..=20` propagate live to exterior `sky.rs` fog
(docs/spec-notes/world.md §6), while non-fog weather traits (`0..=14`) remain
stored-only for this batch.

## Report and lifetime

The actual `script_weather` report remains before `script_sky` and
`sky_follows_game`. It keeps the existing shown/rain update guard, then publishes
one finite sample on every bridge pass even when the shown ID and rain flag stay
the same. Readers consume the last report; they do not predict the next clock
update. Classification comes from the destination WTHR; the outgoing weather
does not supply a second fractional contribution.

[observed: Fallout: New Vegas 1.4.0.525 + xNVSE 6.4.9 + JIP LN 57.30, 2026-11-24,
GSDocMitchellHouse (`0x00103DF9`) and Goodsprings (`0x000CDF51`), 10 official base
masters, isolated staging copy (read-only), committed probe `w4g_04_weather_cold_start`
in `tools/capture/nvse/catalogs/wave4g_f2_extenders.json` (companion catalog PR #3803);
no DLL or source consulted] Immediately on cold start in an interior before ever
visiting an exterior, the original game already has settled `NVDefaultWeather`
(`0x00064609`, Pleasant) active with `GetCurrentWeatherPercent = 1.0`,
`IsPleasant = 1.0`, `IsSnowing = 0`, `IsRaining = 0`, `IsCloudy = 0.0`, and
`GetIsCurrentWeather(cur) = 1`; moving to Goodsprings exterior resolves
`NVGoodspringsWeather` (`0x001237D7`) with the same settled values. Verbatim probe
output:
- `PROBE|w4g_04_weather_cold_start|cold(pct=1.0000,snow=0,rain=0,pls=1.0000,cld=0.0000,cur=00064609,isDef=0,isCur=1)`
- `PROBE|w4g_04_weather_exterior|moved=1`
- `PROBE|w4g_04_weather_exterior|ext(pct=1.0000,snow=0,rain=0,pls=1.0000,cld=0.0000,cur=001237D7,isCur=1)`

[design] When the load order defines vanilla `NVDefaultWeather` (`0x00064609`, WTHR),
`Game::new` initializes `WeatherState` with `NVDefaultWeather` shown and settled at
`1.0` using its authored WTHR `DATA` classification byte (defaulting to Pleasant `1` if
`DATA` is omitted). Synthetic load orders that do not define `0x00064609`, invalidated
samples, legacy two-argument `set_shown_weather` writes, and restored snapshots before
the sky bridge reports remain unmeasured and return zero. A valid sample requires a
shown weather and finite progress in `0..=1`. Invalid progress retires the fraction and
classification together. A fraction-only host report cannot inherit an old
classification. No measurement is added to saves. Pending weather requests retain their
existing `GetIsCurrentWeather` precedence but do not masquerade as a measured sky.

## Authored controls and qualification

The seven original percent controls are retained byte-for-byte as a prefix.
Eight additional shared-App controls use authored WTHR records through the real
importer, real SkyState fade/clock methods, the production reporting helper and
numeric, compiled-source and raw CTDA consumers. They cover classification
freshness, stable/instant/fractional transitions, outgoing/pending neighbors,
cold/restore/legacy state, invalid measurements and report-before-clock cadence.
The test builder is shared with the existing lighting fixtures; it is not loaded
twice under different module names.

The controlled baseline neutrally extracts the original report body and exposes
existing producer/test helpers, with the identical fifteen fixtures. It is not
pristine main and calls no new Game API. Execution results are qualified per exact source and dependency context;
authored coverage alone does not qualify execution. Full ScriptState scheduling,
rendering, native parity and execution of original installed mods remain outside
this source-only evidence.
