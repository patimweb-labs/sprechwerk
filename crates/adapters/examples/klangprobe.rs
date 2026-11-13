// Messhilfe: wie stark senkt RNNoise Rauschen bzw. Stimme?
fn sinus(rate: u32, hz: f32, n: usize, a: f32) -> Vec<f32> {
    (0..n)
        .map(|i| a * (2.0 * std::f32::consts::PI * hz * i as f32 / rate as f32).sin())
        .collect()
}
fn stimme(rate: u32, n: usize, a: f32) -> Vec<f32> {
    // 120 Hz Grundton + Obertöne, mit 4 Hz moduliert (Silben)
    (0..n)
        .map(|i| {
            let t = i as f32 / rate as f32;
            let huelle = 0.5 + 0.5 * (2.0 * std::f32::consts::PI * 4.0 * t).sin();
            let s = (2.0 * std::f32::consts::PI * 120.0 * t).sin()
                + 0.5 * (2.0 * std::f32::consts::PI * 240.0 * t).sin()
                + 0.3 * (2.0 * std::f32::consts::PI * 360.0 * t).sin();
            a * huelle * s / 1.8
        })
        .collect()
}
fn rauschen(n: usize, a: f32) -> Vec<f32> {
    let mut z: u32 = 0x1234_5678;
    (0..n)
        .map(|_| {
            z = z.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
            ((z >> 8) as f32 / (1 << 24) as f32 - 0.5) * 2.0 * a
        })
        .collect()
}
fn rms(b: &[f32]) -> f32 {
    (b.iter().map(|s| s * s).sum::<f32>() / b.len() as f32).sqrt()
}

fn main() {
    use sprechwerk_adapters::klang::Entrauscher;
    let mut e = Entrauscher::neu(48_000);
    let mut r = rauschen(960, 0.05);
    let vor_r = rms(&r);
    for _ in 0..60 {
        let mut b = r.clone();
        e.verarbeite(&mut b);
    }
    e.verarbeite(&mut r);
    println!(
        "Rauschen: {:.4} → {:.4} ({:.0} % bleibt), VAD {:.2}",
        vor_r,
        rms(&r),
        100.0 * rms(&r) / vor_r,
        e.sprachwahrscheinlichkeit()
    );

    let mut e2 = Entrauscher::neu(48_000);
    let mut s = stimme(48_000, 960, 0.3);
    let vor_s = rms(&s);
    for _ in 0..60 {
        let mut b = stimme(48_000, 960, 0.3);
        e2.verarbeite(&mut b);
    }
    e2.verarbeite(&mut s);
    println!(
        "Stimme:  {:.4} → {:.4} ({:.0} % bleibt), VAD {:.2}",
        vor_s,
        rms(&s),
        100.0 * rms(&s) / vor_s,
        e2.sprachwahrscheinlichkeit()
    );

    let mut e3 = Entrauscher::neu(48_000);
    let mut t = sinus(48_000, 500.0, 960, 0.3);
    let vor_t = rms(&t);
    for _ in 0..20 {
        let mut b = sinus(48_000, 500.0, 960, 0.3);
        e3.verarbeite(&mut b);
    }
    e3.verarbeite(&mut t);
    println!(
        "Sinuston: {:.4} → {:.4} ({:.0} % bleibt), VAD {:.2}",
        vor_t,
        rms(&t),
        100.0 * rms(&t) / vor_t,
        e3.sprachwahrscheinlichkeit()
    );
}
