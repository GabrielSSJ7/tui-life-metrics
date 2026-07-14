use anyhow::{anyhow, Result};
use chrono::NaiveDate;

use crate::models::ParsedAction;

/// Build the instruction prompt sent to the Claude CLI for one sentence.
///
/// Today's date is embedded so the model can resolve relative dates
/// ("hoje", "ontem") without a second round-trip.
pub fn build(today: NaiveDate, sentence: &str) -> String {
    format!(
        "Você converte UMA frase de registro de vida em JSON. Hoje é {today}.\n\
         Responda APENAS com um objeto JSON minificado, sem markdown, no formato:\n\
         {{\"category\": string, \"occurred_on\": \"YYYY-MM-DD\", \"attributes\": object, \"note\": string}}\n\
         Regras:\n\
         - category: área da vida, substantivo curto minúsculo no idioma da frase (ex: exercício, vida íntima, trabalho, leitura).\n\
         - occurred_on: resolva datas relativas contra hoje; padrão hoje.\n\
         - attributes: fatos mensuráveis como pares chave/valor. Números como número. Chaves snake_case com unidade quando útil (duration_min, distance_km, count, amount_brl). Objeto vazio se não houver.\n\
         - note: descrição curta e limpa, pode ser vazia.\n\
         Frase: {sentence}"
    )
}

/// Extract the first JSON object from raw CLI stdout and parse it.
///
/// Tolerant of models that wrap output in prose or ```json fences: it slices
/// from the first `{` to the last `}` before deserializing.
pub fn extract(stdout: &str) -> Result<ParsedAction> {
    let start = stdout
        .find('{')
        .ok_or_else(|| anyhow!("no JSON object in claude output: {stdout:?}"))?;
    let end = stdout
        .rfind('}')
        .ok_or_else(|| anyhow!("unterminated JSON in claude output: {stdout:?}"))?;
    if end < start {
        return Err(anyhow!(
            "malformed JSON braces in claude output: {stdout:?}"
        ));
    }
    let slice = &stdout[start..=end];
    serde_json::from_str(slice).map_err(|e| anyhow!("failed to parse action JSON {slice:?}: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::models::AttrValue;

    #[test]
    fn extract_ignores_code_fences_and_prose() {
        let out = "Here you go:\n```json\n{\"category\":\"exercício\",\"occurred_on\":\"2026-07-14\",\"attributes\":{\"duration_min\":30},\"note\":\"corrida\"}\n```\n";
        let action = extract(out).unwrap();
        assert_eq!(action.category, "exercício");
        assert_eq!(action.occurred_on.to_string(), "2026-07-14");
        assert_eq!(
            action.attributes.get("duration_min"),
            Some(&AttrValue::Num(30.0))
        );
        assert_eq!(action.note, "corrida");
    }

    #[test]
    fn extract_defaults_missing_optional_fields() {
        let action = extract("{\"category\":\"leitura\",\"occurred_on\":\"2026-07-14\"}").unwrap();
        assert!(action.attributes.is_empty());
        assert_eq!(action.note, "");
    }

    #[test]
    fn extract_errors_without_json() {
        assert!(extract("desculpe, não entendi").is_err());
    }
}
