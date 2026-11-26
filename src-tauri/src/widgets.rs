use std::collections::HashSet;

use serde_json::{json, Value};

use crate::providers::ToolSpec;
use crate::tools::spec;

pub const TOOL_SHOW_WIDGET: &str = "show_widget";

const KINDS: [&str; 6] = ["chart", "quiz", "drawing", "page", "phone", "bot"];
const CHART_TYPES: [&str; 3] = ["bar", "line", "parts"];
const MAX_SERIES: usize = 8;
const MAX_POINTS: usize = 400;
const MIN_OPTIONS: usize = 2;
const MAX_OPTIONS: usize = 6;
const MAX_QUESTIONS: usize = 30;
const MAX_SVG_BYTES: usize = 400 * 1024;
const MAX_PAGE_BYTES: usize = 1024 * 1024;
const MAX_SCREENS: usize = 60;

const DESCRIPTION: &str = "Показать в ответе живой виджет. Он встаёт в то место ответа, где ты его вызвал: пиши текст до и после вызова как обычно и не пересказывай содержимое виджета словами. \
chart: график по данным. bar сравнивает категории, line показывает изменение во времени, parts делит одно целое на доли (вместо круговой диаграммы). \
quiz: тест с вариантами ответов, пользователь отвечает и видит итог; результат он может прислать в чат. \
drawing: схема, школьный рисунок, чертёж или иллюстрация в SVG. \
page: небольшая интерактивная HTML-страница прямо в чате: калькулятор, тренажёр, наглядное объяснение. \
phone: мобильный сайт или приложение на экране телефона, где можно нажимать. \
bot: кликабельный макет телеграм-бота по описанию. Настоящего бота пользователя проверяй инструментом bot_simulator. \
В drawing, page и phone цвета задавай только CSS-переменными, тогда виджет подходит к светлой и тёмной теме: var(--bg) фон, var(--surface) карточки, var(--line) линии, var(--text) и var(--text-dim) текст, var(--accent), var(--blue), var(--green), var(--red), var(--yellow), var(--purple) и мягкие заливки var(--accent-soft), var(--blue-soft), var(--green-soft), var(--red-soft), var(--yellow-soft), var(--purple-soft). Шрифт var(--font). \
Сеть внутри виджета закрыта: никаких внешних скриптов, шрифтов и картинок, всё пиши внутри.";

pub fn tool_spec() -> ToolSpec {
    let button = json!({ "type": "object", "properties": {
        "text": { "type": "string" },
        "go": { "type": "string", "description": "id экрана, куда ведёт кнопка" },
        "edit": { "type": "boolean", "description": "true меняет сообщение бота на месте вместо нового" },
        "url": { "type": "string", "description": "ссылка вместо перехода" }
    }, "required": ["text"] });
    spec(
        TOOL_SHOW_WIDGET,
        DESCRIPTION,
        json!({ "type": "object", "properties": {
            "kind": { "type": "string", "enum": KINDS },
            "title": { "type": "string", "description": "Короткий заголовок виджета" },
            "chart": { "type": "object", "description": "Для kind chart", "properties": {
                "type": { "type": "string", "enum": CHART_TYPES },
                "labels": { "type": "array", "items": { "type": "string" }, "description": "Подписи по оси X или названия долей" },
                "series": { "type": "array", "items": { "type": "object", "properties": {
                    "name": { "type": "string" },
                    "values": { "type": "array", "items": { "type": "number" } }
                }, "required": ["name", "values"] }, "description": "До 8 рядов, в каждом столько значений, сколько подписей. Для parts один ряд" },
                "unit": { "type": "string", "description": "Единица значений, например ₽ или %" },
                "stacked": { "type": "boolean", "description": "Для bar: складывать ряды в один столбец" }
            }, "required": ["type", "labels", "series"] },
            "questions": { "type": "array", "description": "Для kind quiz", "items": { "type": "object", "properties": {
                "question": { "type": "string" },
                "options": { "type": "array", "items": { "type": "string" }, "description": "От 2 до 6 вариантов" },
                "answer": { "type": "integer", "description": "Номер верного варианта, с нуля" },
                "explain": { "type": "string", "description": "Короткое объяснение после ответа" }
            }, "required": ["question", "options", "answer"] } },
            "svg": { "type": "string", "description": "Для kind drawing: SVG с viewBox, без скриптов" },
            "paper": { "type": "boolean", "description": "Для drawing: тетрадная клетка под рисунком, одна клетка 20 единиц viewBox" },
            "caption": { "type": "string", "description": "Подпись под рисунком" },
            "html": { "type": "string", "description": "Для kind page и phone: HTML с CSS и JS. Высота page подстраивается сама, экран phone 390 на 760. tarn.send(текст) кладёт текст в поле ввода пользователя" },
            "bot": { "type": "object", "description": "Для kind bot", "properties": {
                "name": { "type": "string" },
                "start": { "type": "string", "description": "id первого экрана, его же показывает /start" },
                "fallback": { "type": "string", "description": "Ответ на непонятный текст" },
                "screens": { "type": "array", "items": { "type": "object", "properties": {
                    "id": { "type": "string" },
                    "text": { "type": "string" },
                    "photo": { "type": "string", "description": "Подпись картинки, если сообщение с фото" },
                    "buttons": { "type": "array", "items": { "type": "array", "items": button }, "description": "Кнопки под сообщением, ряды" },
                    "keyboard": { "type": "array", "items": { "type": "array", "items": button }, "description": "Клавиатура вместо поля ввода, ряды" },
                    "next": { "type": "string", "description": "id экрана после любого текста пользователя" }
                }, "required": ["id", "text"] } }
            }, "required": ["name", "start", "screens"] }
        }, "required": ["kind", "title"] }),
    )
}

fn text_of<'a>(value: &'a Value, field: &str) -> &'a str {
    value[field].as_str().map(str::trim).unwrap_or_default()
}

fn check_chart(chart: &Value) -> Result<(), String> {
    let chart_type = text_of(chart, "type");
    if !CHART_TYPES.contains(&chart_type) {
        return Err(format!("Неизвестный вид графика «{chart_type}». Можно: bar, line, parts"));
    }
    let labels = chart["labels"].as_array().ok_or("Нет подписей labels")?;
    if labels.is_empty() || labels.len() > MAX_POINTS {
        return Err(format!("Подписей должно быть от 1 до {MAX_POINTS}"));
    }
    let series = chart["series"].as_array().ok_or("Нет рядов series")?;
    if series.is_empty() || series.len() > MAX_SERIES {
        return Err(format!("Рядов должно быть от 1 до {MAX_SERIES}. Лишнее сложи в «Другое»"));
    }
    if chart_type == "parts" && series.len() != 1 {
        return Err("Для parts нужен ровно один ряд".to_string());
    }
    for row in series {
        let name = text_of(row, "name");
        let values = row["values"].as_array().ok_or_else(|| format!("У ряда «{name}» нет values"))?;
        if values.len() != labels.len() {
            return Err(format!("В ряду «{name}» {} значений, а подписей {}", values.len(), labels.len()));
        }
        if values.iter().any(|value| !value.is_number()) {
            return Err(format!("В ряду «{name}» есть значения не числами"));
        }
        let has_negative = values.iter().filter_map(Value::as_f64).any(|value| value < 0.0);
        if chart_type == "parts" && has_negative {
            return Err("Доли не могут быть отрицательными".to_string());
        }
    }
    Ok(())
}

fn check_quiz(questions: &Value) -> Result<(), String> {
    let questions = questions.as_array().ok_or("Нет вопросов questions")?;
    if questions.is_empty() || questions.len() > MAX_QUESTIONS {
        return Err(format!("Вопросов должно быть от 1 до {MAX_QUESTIONS}"));
    }
    for (index, question) in questions.iter().enumerate() {
        let number = index + 1;
        if text_of(question, "question").is_empty() {
            return Err(format!("У вопроса {number} нет текста"));
        }
        let options = question["options"].as_array().ok_or_else(|| format!("У вопроса {number} нет вариантов"))?;
        if options.len() < MIN_OPTIONS || options.len() > MAX_OPTIONS {
            return Err(format!("У вопроса {number} должно быть от {MIN_OPTIONS} до {MAX_OPTIONS} вариантов"));
        }
        let answer = question["answer"].as_u64().ok_or_else(|| format!("У вопроса {number} не указан верный ответ"))?;
        if answer as usize >= options.len() {
            return Err(format!("У вопроса {number} номер ответа {answer} больше числа вариантов"));
        }
    }
    Ok(())
}

fn check_svg(svg: &str) -> Result<(), String> {
    let lowered = svg.trim_start().to_lowercase();
    if !(lowered.starts_with("<svg") || lowered.starts_with("<?xml")) {
        return Err("svg должен начинаться с тега <svg>".to_string());
    }
    if !lowered.contains("viewbox") {
        return Err("Добавь viewBox, чтобы рисунок масштабировался".to_string());
    }
    if svg.len() > MAX_SVG_BYTES {
        return Err("Рисунок больше 400 КБ".to_string());
    }
    Ok(())
}

fn check_html(html: &str) -> Result<(), String> {
    if html.trim().is_empty() {
        return Err("Нет html".to_string());
    }
    if html.len() > MAX_PAGE_BYTES {
        return Err("Страница больше 1 МБ".to_string());
    }
    Ok(())
}

fn button_targets(rows: &Value) -> Vec<&str> {
    rows.as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_array)
        .flatten()
        .filter_map(|button| button["go"].as_str())
        .collect()
}

fn check_bot(bot: &Value) -> Result<(), String> {
    let screens = bot["screens"].as_array().ok_or("Нет экранов screens")?;
    if screens.is_empty() || screens.len() > MAX_SCREENS {
        return Err(format!("Экранов должно быть от 1 до {MAX_SCREENS}"));
    }
    let mut ids = HashSet::new();
    for screen in screens {
        let id = text_of(screen, "id");
        if id.is_empty() {
            return Err("У экрана нет id".to_string());
        }
        if !ids.insert(id) {
            return Err(format!("Экран {id} повторяется"));
        }
    }
    let start = text_of(bot, "start");
    if !ids.contains(start) {
        return Err(format!("Первого экрана {start} нет среди screens"));
    }
    for screen in screens {
        let id = text_of(screen, "id");
        let mut targets = button_targets(&screen["buttons"]);
        targets.extend(button_targets(&screen["keyboard"]));
        targets.extend(screen["next"].as_str());
        if let Some(missing) = targets.into_iter().find(|target| !ids.contains(target.trim())) {
            return Err(format!("На экране {id} кнопка ведёт на несуществующий экран {missing}"));
        }
    }
    Ok(())
}

pub fn check(input: &Value) -> Result<String, String> {
    let kind = text_of(input, "kind");
    let title = text_of(input, "title");
    if title.is_empty() {
        return Err("Нужен заголовок title".to_string());
    }
    match kind {
        "chart" => check_chart(&input["chart"])?,
        "quiz" => check_quiz(&input["questions"])?,
        "drawing" => check_svg(input["svg"].as_str().unwrap_or_default())?,
        "page" | "phone" => check_html(input["html"].as_str().unwrap_or_default())?,
        "bot" => check_bot(&input["bot"])?,
        other => return Err(format!("Неизвестный вид виджета «{other}». Можно: {}", KINDS.join(", "))),
    }
    let after = match kind {
        "quiz" => " Ответы пользователь может прислать в чат отдельным сообщением.",
        "bot" => " Это макет: настоящий код бота он не запускает.",
        _ => "",
    };
    Ok(format!("Виджет «{title}» показан пользователю в ответе.{after}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_proper_chart_is_accepted() {
        let input = json!({ "kind": "chart", "title": "Расходы", "chart": {
            "type": "bar", "labels": ["Пн", "Вт"], "series": [{ "name": "Opus", "values": [1.5, 2] }]
        } });
        assert!(check(&input).is_ok());
    }

    #[test]
    fn chart_values_must_match_labels() {
        let input = json!({ "kind": "chart", "title": "Расходы", "chart": {
            "type": "line", "labels": ["Пн", "Вт", "Ср"], "series": [{ "name": "Opus", "values": [1, 2] }]
        } });
        assert!(check(&input).unwrap_err().contains("2 значений"));
    }

    #[test]
    fn parts_take_one_series_without_negatives() {
        let two_series = json!({ "kind": "chart", "title": "Доли", "chart": {
            "type": "parts", "labels": ["А"], "series": [{ "name": "x", "values": [1] }, { "name": "y", "values": [2] }]
        } });
        assert!(check(&two_series).is_err());
        let negative = json!({ "kind": "chart", "title": "Доли", "chart": {
            "type": "parts", "labels": ["А", "Б"], "series": [{ "name": "x", "values": [3, -1] }]
        } });
        assert!(check(&negative).is_err());
    }

    #[test]
    fn quiz_answer_must_point_at_an_option() {
        let good = json!({ "kind": "quiz", "title": "Дроби", "questions": [
            { "question": "Больше?", "options": ["1/2", "2/3"], "answer": 1 }
        ] });
        assert!(check(&good).unwrap().contains("прислать в чат"));
        let wrong = json!({ "kind": "quiz", "title": "Дроби", "questions": [
            { "question": "Больше?", "options": ["1/2", "2/3"], "answer": 2 }
        ] });
        assert!(check(&wrong).is_err());
    }

    #[test]
    fn drawing_needs_svg_with_view_box() {
        let good = json!({ "kind": "drawing", "title": "Треугольник", "svg": "<svg viewBox=\"0 0 10 10\"></svg>" });
        assert!(check(&good).is_ok());
        let without_box = json!({ "kind": "drawing", "title": "Треугольник", "svg": "<svg width=\"10\"></svg>" });
        assert!(check(&without_box).is_err());
        let not_svg = json!({ "kind": "drawing", "title": "Треугольник", "svg": "<div></div>" });
        assert!(check(&not_svg).is_err());
    }

    #[test]
    fn bot_buttons_must_lead_to_known_screens() {
        let good = json!({ "kind": "bot", "title": "Запись", "bot": { "name": "Анна", "start": "menu", "screens": [
            { "id": "menu", "text": "Привет", "buttons": [[{ "text": "Записаться", "go": "book" }]] },
            { "id": "book", "text": "Когда удобно?", "next": "menu" }
        ] } });
        assert!(check(&good).is_ok());
        let broken = json!({ "kind": "bot", "title": "Запись", "bot": { "name": "Анна", "start": "menu", "screens": [
            { "id": "menu", "text": "Привет", "buttons": [[{ "text": "Записаться", "go": "nowhere" }]] }
        ] } });
        assert!(check(&broken).unwrap_err().contains("nowhere"));
        let no_start = json!({ "kind": "bot", "title": "Запись", "bot": { "name": "Анна", "start": "x", "screens": [
            { "id": "menu", "text": "Привет" }
        ] } });
        assert!(check(&no_start).is_err());
    }

    #[test]
    fn unknown_kind_and_missing_title_are_refused() {
        assert!(check(&json!({ "kind": "map", "title": "Карта" })).is_err());
        assert!(check(&json!({ "kind": "page", "title": " ", "html": "<p>1</p>" })).is_err());
    }
}
