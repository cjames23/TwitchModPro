# On-device toxicity model

These files are bundled with the app. They're only loaded when **Settings → On-device toxicity check** is on.

| File | What it is |
|---|---|
| `model.safetensors` | MiniLMv2-L6-H384 toxicity classifier, weights converted from float32 to float16 (45 MB) |
| `config.json` | The model's configuration (unchanged) |
| `vocab.txt` | The BERT uncased WordPiece vocabulary (unchanged) |
| `LICENSE` | The Apache License 2.0, which the model is released under |

## Source

- Model: [minuva/MiniLMv2-toxic-jigsaw-lite](https://huggingface.co/minuva/MiniLMv2-toxic-jigsaw-lite), revision `f3440cc98e22ec35b0d11e0e7a8e04b15b69c09f`
- License: Apache-2.0 (per the model card)
- It was fine-tuned on the Jigsaw Toxic Comment Classification dataset, with [unitary/toxic-bert](https://huggingface.co/unitary/toxic-bert) as the teacher. It has two outputs: `toxic` and `severe_toxic` (sigmoid scores from 0 to 1).

## Changes

The only change is the precision of the weights: every tensor in the original `model.safetensors` (float32, SHA-256 `4d4dd329bd76662b3927b6d5280d3415347a196418678cb622d4d76ccfeec1f2`) was converted to float16 with candle. Scores match the original to within about 0.0003. For example, "This is pure trash" scores 0.8866 here, against 0.887 on the model card.

SHA-256 of the bundled files:

```
a5bc720e1684bbc7a8e6d75239c7ad9f356b7e8f1fd067ae53d1cf46523ddeb0  model.safetensors
32cddedf17815d8b25252bf59a86ea0a01cdfac7a5f54087745252c9855bd43f  config.json
07eced375cec144d27c900241f3e339478dec958f92fddbc551f295c992038a3  vocab.txt
```

## Limits

- English only. It's trained on Wikipedia talk-page comments, not Twitch chat, so emote names, copypasta and in-jokes can score oddly. That's why its rules start in test mode.
- It scores the message as written and doesn't undo leetspeak; keyword rules with "lenient matching" cover that.
