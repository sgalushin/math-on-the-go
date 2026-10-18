use crate::model::{AnswerFormat, Kind, Question};

pub fn canonical(id: &str) -> &str {
    id.split("~v~").next().unwrap_or(id)
}
pub fn reconstruct(q: &Question) -> Question {
    if q.kind != Kind::Task {
        return q.clone();
    }
    let Some(t) = &q.template else {
        return q.clone();
    };
    let integer_in = |name: &str, min: f64, max: f64| {
        t.parameters
            .get(name)
            .is_some_and(|v| v.is_finite() && v.fract() == 0.0 && *v >= min && *v <= max)
    };
    let supported = match t.family.as_str() {
        "linear-evaluation" | "linear-equation" => {
            integer_in("a", -9.0, 9.0)
                && t.parameters["a"] != 0.0
                && integer_in("b", -9.0, 9.0)
                && integer_in("x", -9.0, 9.0)
        }
        "arithmetic-sequence" => {
            integer_in("a", -9.0, 9.0) && integer_in("d", -9.0, 9.0) && integer_in("n", 1.0, 8.0)
        }
        "power" => integer_in("base", 1.0, 4.0) && integer_in("exponent", -3.0, 3.0),
        "derivative-at" => {
            integer_in("a", -6.0, 6.0) && integer_in("n", 1.0, 3.0) && integer_in("x", -3.0, 3.0)
        }
        "integral-power" => {
            integer_in("a", -6.0, 6.0)
                && integer_in("n", 0.0, 2.0)
                && integer_in("lower", 0.0, 3.0)
                && integer_in("upper", 0.0, 3.0)
        }
        _ => false,
    };
    if !supported {
        return q.clone();
    }
    let mut out = q.clone();
    let result: Option<(String, f64, String)> = match t.family.as_str() {
        "linear-evaluation" => {
            let (a, b, x) = (t.parameters["a"], t.parameters["b"], t.parameters["x"]);
            Some((
                format!("Evaluate ${a}x+({b})$ when $x={x}$."),
                a * x + b,
                format!("Substitute $x={x}$: ${a}({x})+({b})={}$.", a * x + b),
            ))
        }
        "linear-equation" => {
            let (a, b, x) = (t.parameters["a"], t.parameters["b"], t.parameters["x"]);
            if a == 0.0 {
                None
            } else {
                Some((
                    format!("Solve ${a}x+({b})={}$.", a * x + b),
                    x,
                    format!("Subtract ${b}$, then divide by ${a}$: $x={x}$."),
                ))
            }
        }
        "arithmetic-sequence" => {
            let (a, d, n) = (t.parameters["a"], t.parameters["d"], t.parameters["n"]);
            Some((format!("An arithmetic sequence has first term ${a}$ and common difference ${d}$. Find term ${n}$."),a+(n-1.0)*d,format!("Use $u_n=a+(n-1)d$: ${a}+({n}-1)({d})={}$.",a+(n-1.0)*d)))
        }
        "power" => {
            let (base, exponent) = (t.parameters["base"], t.parameters["exponent"]);
            Some((
                format!("Evaluate ${base}^{{{exponent}}}$."),
                base.powf(exponent),
                format!(
                    "Raise ${base}$ to power ${exponent}$ to obtain ${}$.",
                    base.powf(exponent)
                ),
            ))
        }
        "derivative-at" => {
            let (a, n, x) = (t.parameters["a"], t.parameters["n"], t.parameters["x"]);
            Some((
                format!("For $f(x)={a}x^{{{n}}}$, find $f'({x})$."),
                a * n * x.powf(n - 1.0),
                format!(
                    "The power rule gives $f'(x)={}x^{{{}}}$. Substitute ${x}$ to obtain ${}$.",
                    a * n,
                    n - 1.0,
                    a * n * x.powf(n - 1.0)
                ),
            ))
        }
        "integral-power" => {
            let (a, n, lower, upper) = (
                t.parameters["a"],
                t.parameters["n"],
                t.parameters["lower"],
                t.parameters["upper"],
            );
            if n == -1.0 {
                None
            } else {
                let answer = a / (n + 1.0) * (upper.powf(n + 1.0) - lower.powf(n + 1.0));
                Some((format!("Evaluate $\\int_{{{lower}}}^{{{upper}}}{a}x^{{{n}}}\\,dx$."),answer,format!("An antiderivative is $\\frac{{{a}}}{{{}}}x^{{{}}}$. Evaluate it at ${upper}$ and subtract its value at ${lower}$ to get ${answer}$.",n+1.0,n+1.0)))
            }
        }
        _ => None,
    };
    if let Some((prompt, answer, explanation)) = result.filter(|(_, v, _)| v.is_finite()) {
        out.id = format!("{}~v~0", canonical(&q.id));
        out.prompt = prompt;
        out.answer = answer.to_string();
        out.accepted_answers.clear();
        out.explanation = explanation;
        out.answer_format = AnswerFormat::Text;
        out.choices.clear();
        out.graph = None;
        out.hints = vec![match t.family.as_str() {
            "linear-equation" => "Undo the addition, then undo the multiplication.",
            "linear-evaluation" => {
                "Replace each x with its specified value, then use order of operations."
            }
            "arithmetic-sequence" => {
                "Use first term plus (term number minus one) times the common difference."
            }
            "derivative-at" => "Use the power rule, then substitute the evaluation point.",
            "integral-power" => "Integrate using the power rule and evaluate upper minus lower.",
            _ => "Apply the definition of an integer power.",
        }
        .into(), match t.family.as_str() {
            "linear-equation" => "Subtract the constant term from both sides first.",
            "linear-evaluation" => "Do the multiplication before adding the constant.",
            "arithmetic-sequence" => "The number of equal steps is one less than the term number.",
            "derivative-at" => "For a x^n, the derivative is a n x^(n-1).",
            "integral-power" => "Increase the exponent by one and divide by the new exponent.",
            _ => "For a negative exponent, first calculate the positive power, then take its reciprocal.",
        }.into()];
    }
    out
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn canonical_variant_identity() {
        assert_eq!(canonical("q~v~abcd"), "q");
        assert_eq!(canonical("q"), "q");
    }
}
