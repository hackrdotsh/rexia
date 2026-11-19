use serde::Deserialize;

#[derive(Clone, Debug, Deserialize)]
pub struct Request {
    pub call_id: String,
    pub prompt: Prompt,
}

#[derive(Clone, Debug, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Prompt {
    Input(Fields),
    Pick(Picker),
}

#[derive(Clone, Debug, Deserialize)]
pub struct Fields {
    pub title: String,
    #[serde(default)]
    pub placeholder: String,
    #[serde(default)]
    pub value: String,
    pub confirm: Option<String>,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Picker {
    #[serde(flatten)]
    pub fields: Fields,
    pub items: Vec<Item>,
    #[serde(default)]
    pub multiple: bool,
    #[serde(default)]
    pub allow_other: bool,
}

#[derive(Clone, Debug, Deserialize)]
pub struct Item {
    pub id: String,
    pub title: Option<String>,
    pub subtitle: Option<String>,
    pub icon: Option<String>,
    pub detail: Option<String>,
    #[serde(default)]
    pub selected: bool,
}

impl Item {
    pub fn title(&self) -> &str {
        self.title.as_deref().unwrap_or(&self.id)
    }
}

impl Prompt {
    pub fn fields(&self) -> &Fields {
        match self {
            Self::Input(fields) => fields,
            Self::Pick(picker) => &picker.fields,
        }
    }
}
