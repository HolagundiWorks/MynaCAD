//! Form descriptions for the WinUI dialog. The field list is the engine's; this only writes it as JSON.

use aad_engine::Session;
use aad_engine::forms::{self, Kind, Len};
use serde_json::{Value, json};

/// JSON for the settings dialog of `id`, or `None` when the command has no dialog fields.
pub fn form_json(session: &Session, id: &str) -> Option<String> {
    let form = forms::get(id.trim())?;
    form.dialog_fields().next()?;
    let prefill = forms::prefill(session, form);
    let drawn = session.doc().ok().and_then(aad_engine::annot::drawing_unit);
    let mut rooms = Value::Null;
    let fields: Vec<Value> = form
        .fields
        .iter()
        .map(|field| {
            // A length can be typed in any unit; "drawing units" names the unit this drawing is in.
            let unit_name = match field.len {
                Some(Len::Drawing) => drawn.map(|u| u.name()),
                Some(Len::Mm) => Some("mm"),
                Some(Len::M) => Some("m"),
                None => None,
            };
            let label = match (field.len, drawn) {
                (Some(Len::Drawing), Some(u)) => field.label.replace("drawing units", u.name()),
                _ => field.label.to_string(),
            };
            let help = if field.len.is_some() {
                format!("{} A bare number is in {}; a unit can be typed: 12in, 300mm, 0.3m, 1'6\".", field.help, unit_name.unwrap_or("drawing units"))
                    .trim()
                    .to_string()
            } else {
                field.help.to_string()
            };
            let mut value = json!({
                "key": field.key,
                "label": label,
                "default": field.default,
                "required": field.required,
                "help": help,
                "pick": field.is_pick(),
                "value": prefill.get(field.key).cloned().unwrap_or(Value::Null),
            });
            let kind = match field.kind {
                Kind::Text => "text",
                Kind::Number => "number",
                Kind::Numbers => "numbers",
                Kind::Bool => "bool",
                Kind::Choice(choices) => {
                    value["choices"] = json!(choices);
                    "choice"
                }
                Kind::RoomType => {
                    if rooms.is_null() {
                        rooms = json!(forms::room_types().iter().map(|(code, name)| json!({ "code": code, "name": name })).collect::<Vec<_>>());
                    }
                    "room"
                }
                Kind::Point => "point",
                Kind::Points => "points",
                Kind::Table(columns) => {
                    value["columns"] = json!(
                        columns
                            .iter()
                            .map(|column| json!({ "key": column.key, "label": column.label, "numeric": column.numeric }))
                            .collect::<Vec<_>>()
                    );
                    "table"
                }
            };
            value["kind"] = json!(kind);
            value
        })
        .collect();
    Some(
        json!({
            "id": form.id,
            "title": form.title,
            "wrap": form.wrap,
            "fields": fields,
            "rooms": rooms,
        })
        .to_string(),
    )
}
