install -d /opt/agent-computer
cat >/opt/agent-computer/scenarios.yaml <<'SCENARIOS'
fallback:
  content: 'Try "research example.com", "write about ubuntu" or "test example.org".'
scenarios:
  - name: research
    match: '(?i)research example\.com'
    turns:
      - tool_calls:
          - name: Computer
            args:
              action: screenshot
      - tool_calls:
          - name: Computer
            args:
              action: key
              combo: ctrl+l
      - tool_calls:
          - name: Computer
            args:
              action: type
              text: https://example.com
      - tool_calls:
          - name: Computer
            args:
              action: key
              combo: enter
      - tool_calls:
          - name: Computer
            args:
              action: screenshot
      - content: I opened https://example.com on my computer and read the page.
  - name: write
    match: '(?i)write about ubuntu'
    turns:
      - tool_calls:
          - name: Computer
            args:
              action: screenshot
      - tool_calls:
          - name: Computer
            args:
              action: key
              combo: ctrl+l
      - tool_calls:
          - name: Computer
            args:
              action: type
              text: https://ubuntu.com
      - tool_calls:
          - name: Computer
            args:
              action: key
              combo: enter
      - tool_calls:
          - name: Computer
            args:
              action: screenshot
      - content: I opened https://ubuntu.com on my computer to gather notes for the article.
  - name: test
    match: '(?i)test example\.org'
    turns:
      - tool_calls:
          - name: Computer
            args:
              action: screenshot
      - tool_calls:
          - name: Computer
            args:
              action: key
              combo: ctrl+l
      - tool_calls:
          - name: Computer
            args:
              action: type
              text: https://example.org
      - tool_calls:
          - name: Computer
            args:
              action: key
              combo: enter
      - tool_calls:
          - name: Computer
            args:
              action: screenshot
      - content: I loaded https://example.org on my computer and the page renders.
SCENARIOS
