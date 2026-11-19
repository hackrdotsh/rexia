rex.action{
  name = "prompt_text_fixture",
  title = "Prompt text fixture",
  run = function()
    local answer, reason = rex.ui.input{ title = "Enter a note", value = "draft", confirm = "Save" }
    return { answer = answer, reason = reason }
  end,
}

rex.action{
  name = "prompt_pick_fixture",
  title = "Prompt pick fixture",
  run = function()
    local answer, reason = rex.ui.pick{
      title = "Choose a file",
      items = {{ id = "first", title = "First file", icon = "document", subtitle = "docs", detail = "new" }, "second"},
    }
    return { answer = answer, reason = reason }
  end,
}

rex.action{
  name = "prompt_multiple_fixture",
  title = "Prompt multiple fixture",
  run = function()
    local answer, reason = rex.ui.pick{
      title = "Choose files",
      multiple = true,
      items = {{ id = "first", selected = true }, "second", "third"},
      confirm = "Stage",
    }
    return { answer = answer, reason = reason }
  end,
}
