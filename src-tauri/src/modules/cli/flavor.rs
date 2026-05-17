use std::time::{Duration, SystemTime, UNIX_EPOCH};

fn tick() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(42)
}

fn pick_str(options: &[&'static str]) -> &'static str {
    options[(tick() as usize) % options.len()]
}

/// Pick between two short phrases (deterministic per nanosecond tick).
pub fn fun_pair(a: &'static str, b: &'static str) -> String {
    if (tick() as usize).is_multiple_of(2) {
        a.into()
    } else {
        b.into()
    }
}

/// Main spinner label — one funny invented word shown as a present participle.
/// Format in the spinner: `⠹ Thonking · 4s`
pub fn thinking_label() -> &'static str {
    pick_str(&[
        // ── classic hacker lore ──
        "Grokking",    // deep understanding (Heinlein)
        "Frobnifying", // frobnicate = to fiddle with knobs
        "Quuxing",     // quux: the fourth hacker placeholder
        "Thonking",    // thinking + honk
        // ── sounds like real work ──
        "Cogitating",    // stuffy but correct
        "Percolating",   // ideas slowly brewing
        "Woolgathering", // absent-minded musing
        "Deliberating",  // weighing all options solemnly
        "Ruminating",    // chewing on the problem
        "Pontificating", // making it sound important
        "Extrapolating", // going beyond the data boldly
        "Triangulating", // finding the answer from three bad clues
        "Defragmenting", // classic Windows nostalgia
        "Overclocking",  // pushing silicon to its limits
        "Compiling",     // it's always compiling
        "Initialising",  // British spelling for extra gravitas
        // ── invented nonsense ──
        "Blorping",
        "Wibbling",
        "Zorbulating",
        "Kerfluffling", // from kerfuffle
        "Discombobulating",
        "Schmozzling",
        "Glonking",
        "Flerbulating",
        "Blorbinating",
        "Slorbing",
        "Murgling",
        "Gnurfling",
        "Zippulating",
        "Snorkling",
        "Wuffling",
        "Schmoogling",
        "Grumpulating",
        "Boffinating", // boffin = British clever scientist
        "Tronking",    // Tron cinematic universe
        "Noodling",    // loosely brainstorming
        "Simmering",   // on low heat
        "Bewildering", // turning confusion into answers
        "Splorching",
        "Frumbling",
        "Zibulating",
        "Plonkulating",
        "Greebling",     // greeble: tiny surface details on a spaceship
        "Yak-shaving",   // solving the meta-problem of the meta-problem
        "Wiggling",      // sometimes you just need to wiggle it
        "Bamboozling",   // bamboozle reversed
        "Spelunking",    // exploring deep caves of context
        "Untangling",    // knots, conceptual and otherwise
        "Manifolding",   // higher-dimensional problem solving
        "Vibing",        // when the model just knows
        "Debugging",     // always, everywhere
        "Hallucinating", // (briefly, before correcting itself)
    ])
}

/// Shown under the banner when the REPL starts (dim line).
pub fn repl_tagline() -> &'static str {
    pick_str(&[
        "The terminal is a stage; we are all merely agents.",
        "Slippery when wet: undefined behavior ahead.",
        "MCP tools: use as directed. Side effects may include working code.",
        "If the model hallucinates, blame the temperature — or Mercury retrograde.",
        "Every directory_tree is a tiny forest walk for your CPU.",
        "You, me, and a context window to go.",
        "Ship it? Ship it. (After the tests pass.)",
        "Pro tip: `ollama serve` is the real sidecar.",
    ])
}

/// Footer after a turn finishes — keeps “Baked” but adds a rotating quip.
pub fn baked_message(elapsed: Duration, fmt_elapsed: impl Fn(Duration) -> String) -> String {
    let t = fmt_elapsed(elapsed);
    let quip = pick_str(&[
        "still warm",
        "chef's kiss",
        "golden brown",
        "soup's on",
        "take it off the heat",
        "do not eat the CLI output",
        "proof-of-work complete",
        "you earned this reply",
        "cool enough to ship",
        "crispy edges included",
    ]);
    format!("Baked for {t} — {quip}")
}

/// Variants for “N tool call(s)” status lines.
pub fn tool_batch_label(n: usize) -> String {
    match n {
        0 => pick_str(&[
            "Preparing tools…",
            "Lining up the wrenches…",
            "Waking the toolbox…",
            "Checking who brought extensions…",
        ])
        .to_string(),
        1 => pick_str(&[
            "Running one tool…",
            "Unleashing one tool…",
            "Single-tool rodeo…",
            "One tool, hold the mayo…",
        ])
        .to_string(),
        _ => pick_str(&[
            "Running {n} tools…",
            "Deploying {n} gadgets…",
            "Spinning up {n} helpers…",
            "{n} tools walk into a bar…",
        ])
        .replace("{n}", &n.to_string()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thinking_label_nonempty() {
        assert!(!thinking_label().is_empty());
    }

    #[test]
    fn baked_message_has_elapsed_and_quip() {
        let s = baked_message(Duration::from_millis(500), |d| {
            if d.as_millis() < 1000 {
                format!("{}ms", d.as_millis())
            } else {
                "nope".into()
            }
        });
        assert!(s.contains("500ms"), "{s}");
        assert!(s.contains("Baked for"), "{s}");
        assert!(s.contains('—'), "{s}");
    }

    #[test]
    fn tool_batch_plural_has_number() {
        let s = tool_batch_label(3);
        assert!(s.contains('3'), "{s}");
    }
}
