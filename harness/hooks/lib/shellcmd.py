"""Blanking keeps offsets for regex command-position anchors; tokenizing splits with shlex."""

import os
import re
import shlex

_HEREDOC_START = re.compile(r"<<-?\s*(['\"]?)(\w+)\1")
_HEREDOC_BODY = re.compile(r"<<-?\s*(['\"]?)(\w+)\1[^\n]*\n(.*?)^[ \t]*\2[ \t]*$", re.S | re.M)
SEPARATORS = ";&|()\n"
_ASSIGNMENT = re.compile(r"^[A-Za-z_][A-Za-z0-9_]*=")
_DURATION = re.compile(r"^[0-9.]+[smhd]?$")


def rewrite_input(data, field, value, context):
    return {"hookSpecificOutput": {
        "hookEventName": "PreToolUse",
        "permissionDecision": "allow",
        "updatedInput": dict(data["tool_input"], **{field: value}),
        "additionalContext": context,
    }}


def argv_spans(text):
    tokens = []
    index = 0

    def quoted(cursor, quote):
        cursor += 1
        while cursor < len(text):
            if text[cursor] == quote:
                return cursor + 1
            if quote == '"' and text[cursor] in "$`":
                raise ValueError("unsupported expansion")
            cursor += 2 if quote == '"' and text[cursor] == "\\" else 1
        raise ValueError("unclosed quote")

    def grouped(cursor):
        nesting = 1
        cursor += 1
        while cursor < len(text):
            char = text[cursor]
            if char in "'\"":
                cursor = quoted(cursor, char)
                continue
            if char == "\\":
                cursor += 2
                continue
            if char == "(":
                nesting += 1
            elif char == ")":
                nesting -= 1
                if not nesting:
                    return cursor + 1
            cursor += 1
        raise ValueError("unclosed substitution")

    while index < len(text):
        if text[index] in " \t\r":
            index += 1
            continue
        start = index
        if text[index] in SEPARATORS:
            index += 1
            tokens.append(("sep", start, index))
            continue
        while index < len(text) and text[index] not in " \t\r" + SEPARATORS:
            char = text[index]
            if char in "'\"":
                index = quoted(index, char)
            elif char == "\\":
                if index + 1 >= len(text):
                    raise ValueError("unclosed escape")
                index += 2
            elif text.startswith("<(", index):
                index = grouped(index + 1)
            elif char in "<>`$#":
                raise ValueError("unsupported shell operator")
            else:
                index += 1
        tokens.append(("word", start, index))
    return tokens


def blank(text):
    return "".join(c if c == "\n" else " " for c in text)


def strip_heredocs(s):
    out = []
    i, n = 0, len(s)
    while i < n:
        m = _HEREDOC_START.search(s, i)
        if not m:
            out.append(s[i:])
            break
        out.append(s[i:m.end()])
        line_end = s.find("\n", m.end())
        if line_end == -1:
            out.append(s[m.end():])
            break
        out.append(s[m.end():line_end + 1])
        body_start = line_end + 1
        term = re.compile(r"^[ \t]*" + re.escape(m.group(2)) + r"[ \t]*$", re.M)
        tm = term.search(s, body_start)
        if tm:
            out.append(blank(s[body_start:tm.start()]))
            i = tm.start()
        else:
            out.append(blank(s[body_start:]))
            i = n
    return "".join(out)


def strip_quotes(s):
    out = []
    i, n = 0, len(s)
    while i < n:
        c = s[i]
        if c == "'":
            j = s.find("'", i + 1)
            end = n if j == -1 else j + 1
            out.append(blank(s[i:end]))
            i = end
            continue
        if c == '"':
            j = i + 1
            while j < n:
                if s[j] == "\\" and j + 1 < n:
                    j += 2
                    continue
                if s[j] == '"':
                    j += 1
                    break
                j += 1
            out.append(blank(s[i:j]))
            i = j
            continue
        out.append(c)
        i += 1
    return strip_prefixes("".join(out))


def words(cmd):
    text = _HEREDOC_BODY.sub(lambda m: m.group(0)[: m.start(3) - m.start(0)] + m.group(2), cmd)
    try:
        lexer = shlex.shlex(text, posix=True, punctuation_chars=SEPARATORS)
        lexer.whitespace = " \t\r"
        lexer.whitespace_split = True
        lexer.commenters = "#"
        return list(lexer)
    except ValueError:
        return None


def is_separator(token):
    return bool(token) and set(token) <= set(SEPARATORS)


def segments(tokens):
    segment = []
    for token in tokens + [";"]:
        if is_separator(token):
            yield segment
            segment = []
        else:
            segment.append(token)


def command(segment, prefixes):
    prefixes = set(prefixes) | {"command", "sudo", "doas"}
    wrapper = None
    index = 0
    while index < len(segment):
        word = segment[index]
        name = os.path.basename(word)
        if wrapper and word in {"-u", "--user", "-g", "--group", "-C", "--chdir", "--unset"}:
            index += 2
            continue
        if _ASSIGNMENT.match(word) or word == "--" or (word.startswith("-") and index > 0):
            index += 1
            continue
        if name in prefixes:
            wrapper = name
            index += 1
            if name == "timeout" and index < len(segment) and _DURATION.match(segment[index]):
                index += 1
            continue
        break
    return index


_PREFIXES = {"sudo", "doas", "command", "env", "exec", "builtin", "nohup", "setsid"}


def strip_prefixes(text):
    out = list(text)
    for match in re.finditer(r"(?:^|[\n;&|({`])((?:\$\{[^$(){};&|`\n]*\}|[^\n;&|(){}`])*)", text):
        span = match.group(1)
        tokens = list(re.finditer(r"\S+", span))
        index = command([token.group() for token in tokens], _PREFIXES)
        if index:
            end = tokens[min(index, len(tokens)) - 1].end()
            start = match.start(1)
            out[start:start + end] = blank(span[:end])
    return "".join(out)


def tokenize(text, separators=";&|\n`", escaped_quoted=True, groups=True, whitespace=" \t\r"):
    tokens = []
    buffer = []
    quoted = False
    index = 0

    def flush():
        nonlocal buffer, quoted
        if buffer:
            tokens.append(("word", "".join(buffer), quoted))
        buffer = []
        quoted = False

    while index < len(text):
        char = text[index]
        if char == "\\" and index + 1 < len(text):
            buffer.append(text[index + 1])
            quoted = quoted or escaped_quoted
            index += 2
            continue
        if char == "'":
            end = text.find("'", index + 1)
            end = len(text) if end < 0 else end
            buffer.append(text[index + 1:end])
            quoted = True
            index = end + 1
            continue
        if char == '"':
            cursor = index + 1
            piece = []
            while cursor < len(text):
                if text[cursor] == "\\" and cursor + 1 < len(text):
                    piece.append(text[cursor + 1])
                    cursor += 2
                    continue
                if text[cursor] == '"':
                    break
                piece.append(text[cursor])
                cursor += 1
            buffer.append("".join(piece))
            quoted = True
            index = cursor + 1
            continue
        if char in separators:
            flush()
            tokens.append(("sep", char, False))
            index += 1
            continue
        if groups and (char == ")" or (char == "(" and (not buffer or buffer[-1] == "$"))):
            if buffer and buffer[-1] == "$" and char == "(":
                buffer.pop()
            flush()
            tokens.append(("sep", char, False))
            index += 1
            continue
        if char in whitespace:
            flush()
            index += 1
            continue
        buffer.append(char)
        index += 1
    flush()
    return tokens



def shell_segments_sep(text):
    segments, buf = [], []
    quote = None
    i = 0
    while i < len(text):
        char = text[i]
        if quote:
            buf.append(char)
            if quote == '"' and char == "\\" and i + 1 < len(text):
                i += 1
                buf.append(text[i])
            elif char == quote:
                quote = None
            i += 1
            continue
        if char in ("'", '"'):
            quote = char
            buf.append(char)
            i += 1
            continue
        if char == "\\" and i + 1 < len(text):
            buf.append(char)
            i += 1
            buf.append(text[i])
            i += 1
            continue
        if char in ";|&\n":
            sep = ";" if char == "\n" else char
            if i + 1 < len(text) and text[i:i + 2] in ("&&", "||"):
                sep = text[i:i + 2]
                i += 1
            segment = "".join(buf).strip()
            if segment:
                segments.append((segment, sep))
            buf = []
            i += 1
            continue
        buf.append(char)
        i += 1
    segment = "".join(buf).strip()
    if segment:
        segments.append((segment, None))
    return segments



def command_index(tokens):
    i = 0
    wrappers = {"env", "command", "builtin", "exec", "nohup", "sudo", "time"}
    controls = {"!", "{", "if", "then", "elif", "while", "until", "do"}
    value_flags = {
        "env": {"-u", "--unset", "-C", "--chdir", "-S", "--split-string"},
        "sudo": {
            "-u", "--user", "-g", "--group", "-h", "--host", "-p",
            "--prompt", "-C", "--chdir", "-r", "--role", "-t", "--type",
        },
    }
    while i < len(tokens):
        word = os.path.basename(tokens[i]).lstrip("(")
        if re.match(r"^[A-Za-z_][A-Za-z0-9_]*=", tokens[i]):
            i += 1
            continue
        if word in controls:
            i += 1
            continue
        if word not in wrappers:
            break
        i += 1
        consumes = value_flags.get(word, set())
        while i < len(tokens):
            arg = tokens[i]
            if re.match(r"^[A-Za-z_][A-Za-z0-9_]*=", arg):
                i += 1
            elif arg in consumes and i + 1 < len(tokens):
                i += 2
            elif arg == "--":
                i += 1
                break
            elif arg.startswith("-"):
                i += 1
            else:
                break
    return i



def tripwire_word(token):
    word = token.removeprefix('(').removeprefix('{')
    for char in ('"', "'"):
        word = word.removeprefix(char).removesuffix(char)
    word = os.path.basename(word.removesuffix(')'))
    if word == '-u':
        return '_skip_user'
    if (word in {'sudo', 'doas', 'command', 'builtin', 'nohup', 'nice', 'ionice', 'stdbuf', 'eval', 'exec', 'time', 'timeout', 'env', '', '{', '}', '(', ')', 'if', 'then', 'elif', 'else', 'fi', 'do', 'done', 'while', 'until', 'for', '!'}
            or word.startswith('-') or (bool(word) and word[0] in "0123456789") or re.match(r'^[A-Za-z_].*=', word)):
        return ''
    return word


def tripwire_tokens(text):
    marker = '\x04'
    while marker in text:
        marker += '\x04'
    quoted = {}
    pieces = []
    index = 0
    while index < len(text):
        char = text[index]
        if char == '\\' and index + 1 < len(text):
            pieces.append(text[index:index + 2])
            index += 2
            continue
        if char in ("'", '"'):
            end = index + 1
            while end < len(text):
                if char == '"' and text[end] == '\\' and end + 1 < len(text):
                    end += 2
                elif text[end] == char:
                    end += 1
                    break
                else:
                    end += 1
            value = text[index:end]
            if char == "'" or ('$' + '(' not in value and '`' not in value):
                key = marker + str(len(quoted)) + marker
                quoted[key] = value
                pieces.append(key)
            else:
                pieces.append(value)
            index = end
            continue
        pieces.append(char)
        index += 1
    text = ''.join(pieces)
    text = text.replace("\\\n", " ").replace("\n", " ; ").replace("\t", " ")
    text = text.replace("$(", "\x02 ; ( ")
    opening = True
    while "`" in text:
        text = text.replace("`", "\x02 ; ( " if opening else " ) ; ", 1)
        opening = not opening
    text = text.replace("&&", " \x01 ").replace("||", " \x03 ")
    text = text.replace(";", " ; ").replace("|", " | ").replace("&", " ; ")
    text = text.replace("\x01", "&&").replace("\x03", "||")
    tokens = [word for word in re.split(r"[ \t\n]+", text) if word]
    for index, token in enumerate(tokens):
        for key, value in quoted.items():
            token = token.replace(key, value)
        tokens[index] = token
    return tokens


if __name__ == '__main__':
    import sys
    if sys.argv[1:2] == ['--tripwire']:
        for token in tripwire_tokens(sys.stdin.read()):
            sys.stdout.write(token + '\0' + tripwire_word(token) + '\0')
