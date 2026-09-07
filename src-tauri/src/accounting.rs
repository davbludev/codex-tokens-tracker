//! Pure acceptance rules for the evidence-supported non-resetting modern stream.
use crate::adapter::Tokens;

pub fn reconcile(
    usage: &Tokens,
    endpoint: &Tokens,
    previous: Option<&Tokens>,
) -> Result<bool, &'static str> {
    let delta = usage
        .values()
        .ok_or("Missing reconciliation category; usage unavailable")?;
    let end = endpoint
        .values()
        .ok_or("Missing endpoint category; usage unavailable")?;
    for values in [delta, end] {
        if values.iter().any(|n| *n < 0) {
            return Err("Negative token counter; unsupported stream");
        }
        if values[0].checked_add(values[3]) != Some(values[5])
            || values[1] > values[0]
            || values[4] > values[3]
        {
            return Err("Unsupported token category relationship");
        }
    }
    if let Some(previous) = previous {
        let start = previous.values().ok_or("Previous endpoint unavailable")?;
        for i in 0..6 {
            if end[i] < start[i] {
                return Err("Decreasing thread endpoint; unsupported stream");
            }
            if end[i] - start[i] != delta[i] {
                return Err("Unexplained thread gap; remaining source usage unavailable");
            }
        }
        Ok(false)
    } else {
        if (0..6).any(|i| end[i] < delta[i]) {
            return Err("Usage exceeds opening endpoint");
        }
        Ok(end != delta)
    }
}
