# GreeterContext

## Purpose

GreeterContext returns the message it was given.

## Role Players

| Role player | Why involved | Expected behaviour |
|---|---|---|
| message | Printable message value | Uses `Message` and provides the text getter |

## Role Methods

### message

- **text**
  Signature: `text(&self) -> &String`

## Props

| Prop | Meaning | Notes |
|---|---|---|

## Functionalities

### new

| Started by | Uses | Result |
|---|---|---|
| app startup | message | a greeter context is created |

Rules:
- Signature: `new(message: Message) -> Self`

| Given | When | Then |
|---|---|---|
| a message exists | new is called | a greeter context is created |

### render

| Started by | Uses | Result |
|---|---|---|
| app | message | `&String` |

Rules:
- Signature: `render(&self) -> &String`
- Return `message.text()`.

| Given | When | Then |
|---|---|---|
| a message exists | render is called | the stored message text is returned |
