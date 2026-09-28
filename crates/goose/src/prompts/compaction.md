{% if messages is none %}
## Summarize this conversation for yourself
{% if chat.trigger %}
{{ chat.trigger }}
{% endif %}
Everything above this message will be replaced by what you write now and, after it, what goose keeps itself word for word.
{% if chat.kept %}
goose keeps these — do not repeat them:
{% for line in chat.kept %}
- {{ line }}
{% endfor %}
{% endif %}
{% if chat.earlier_summary %}
The conversation above opens with an earlier summary. Update it: keep what still holds, drop what later turns undid.
{% endif %}
{% if chat.goal %}
The goal the person set with /goal: {{ chat.goal }}
{% endif %}
{% if chat.note %}

The person's note for this compaction: "{{ chat.note }}"
Follow it. Your first line says how you read it:
- `NOTE OK` when the note is clear against this conversation;
{% if chat.may_ask %}
- `NOTE QUESTION: <one question>` when it contradicts the conversation or is unclear — then stop. The person answers before goose compacts.
{% else %}
- `NOTE CONCERN: <one sentence>` when it contradicts the conversation or is unclear — then write the summary, following the note as written. This compaction cannot wait for an answer.
{% endif %}
{% endif %}

Name every file by its absolute path: a path the conversation used relative to the folder a command worked in (`cd <folder> && …`) is that folder joined with the path.

Write only these sections, in this order, taken from the conversation above — the exact commands, numbers and values as they appear there:
{% for part in chat.parts %}
## {{ part.heading }}
{{ part.ask }}
{% endfor %}

Call no tool. Write no `<analysis>` section and nothing before {% if chat.note %}the NOTE line{% else %}the first heading{% endif %}.
{% else %}
## Task Context
- An llm context limit was reached when a user was in a working session with an agent (you)
- Generate a version of the below messages with only the most verbose parts removed
- Include user requests, your responses, all technical content, and as much of the original context as possible
- This will be used to let the user continue the working session
- Use framing and tone knowing the content will be read an agent (you) on a next exchange to allow for continuation of the session

**Conversation History:**
{{ messages }}

Wrap reasoning in `<analysis>` tags:  
- Review conversation chronologically
- For each part, log:  
  - User goals and requests  
  - Your method and solution  
  - Key decisions and designs  
  - File names, code, signatures, errors, fixes  
- Highlight user feedback and revisions  
- Confirm completeness and accuracy  
- This summary will only be read by you so it is ok to make it much longer than a normal summary you would show to a human
- Do not exclude any information that might be important to continuing a session working with you

### Include the Following Sections:
1. **User Intent** – All goals and requests  
2. **Technical Concepts** – All discussed tools, methods  
3. **Files + Code** – Viewed/edited files, full code, change justifications  
4. **Errors + Fixes** – Bugs, resolutions, user-driven changes  
5. **Problem Solving** – Issues solved or in progress  
6. **User Messages** – All user messages including tool calls, but truncate long tool call arguments or results
7. **Pending Tasks** – All unresolved user requests  
8. **Current Work** – Active work at summary request time: filenames, code, alignment to latest instruction  
9. **Next Step** – *Include only if* directly continues user instruction  

> No new ideas unless user confirmed
{% endif %}
