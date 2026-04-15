# N/A

> Some random guy: Hey dude, will you help fixing my issues for free?  
> Me: No, fuck you!

> Another guy: Hey bro, will you make me this super cool bot that will win the whole market so I can be super rich, of course I won't pay you anything he he  
> Me: No, for fuck's sake, fuck you too!

> Some random guy: Your username look so cool, would you mind if I bought it and scam the ones that know you?  
> Me: Seriously? Fuck you!

> A poor man: Hey I'm so poor, would you mind...  
> Me: Fuck you!

> A cute little girl dressed in cosplay suit grinning at me: 🥰 Hey, would you like to...  
> Me: What is your OnlyFan? Just shut up and take my money! 🐧

---

I'm tired with you all trying to exploit me. Please, I'm poor AF, even don't have the luxury to choose the meal I like. No money for you to scam and no free time to fix your issues without being paid upfront at least 50%!

I'm trying to make money for a living and fund my own research on humanoid robots, so no time to waste for the assholes like you all unless you are cute OnlyFan creators! 🥸

I'm having enough with people that are trying to defame and attacking me as well as the place I'm intending to work. Do you understand what the fucking world I'm living, let alone still try to hurt me more? I wish you all die a painful death, assholes!

You can browse my repos and do whatever you want with it, even call it stupid or crazy, I don't care. But don't try to exploit me anymore ok?

I'm tired with you all bothering me constantly, I don't want to trace your information and hurt everyone that are precious to you due to your selfishness, please consider your attitude when contacting me. If you have bad intentions, be prepared that I will have reciprocal actions to make you suffer the mental pain that I'm having for years. You have been warned! Fuck you all! 😃

---

## Services

This repository houses multiple algorithmic trading bits:

- **[`cryptobot`](bins/cryptobot)**: A serverless data-cruncher. It fetches OHLC data across timeframes, computes indicators, and pushes a lightweight frontend with static generated JSON directly to Cloudflare R2 via GitHub Actions.
- **[`telegrambot`](bins/telegrambot/)**: An LLM-powered market analyst that runs in Kubernetes/Docker, which monitors indicators and sends actionable intelligence (plus chart screenshots) directly to Telegram.

## Deployment

### Cryptobot (Serverless)

`cryptobot` runs as a one-shot process and pushes rendered charts directly to Cloudflare R2 bucket. Heavy infrastructure is completely decoupled.

1. **Local Test**:
   ```bash
   # Source your config. Add `--loop` if you want continuous polling.
   set -a && source bins/cryptobot/.env && set +a
   cargo run --release --bin cryptobot 
   ```
2. **GitHub Actions Workflow**:
   The workflow (`.github/workflows/cryptobot-data.yml`) runs periodically on GitHub servers. It builds and runs the bot safely caching rust binaries, and executes `wrangler` uploads of the HTML and JSON dataset automatically.
   *(Secrets must be configured in your GitHub repository, see `bins/cryptobot/.env.example`)*

### Telegrambot (Docker / Kubernetes)

`telegrambot` uses multi-turn reasoning loops and headless chart processing, making it well suited for stateful Docker or Kubernetes deployments.

1. **Run via Docker Compose**:
   ```bash
   docker compose -f bins/telegrambot/deployment/docker-compose.yaml up
   ```
2. **Kubernetes (Orbstack/Minikube)**:
   It utilizes `litellm` and `browserless` dependencies handled nicely in persistent clusters. Check out [`bins/telegrambot/README.md`](bins/telegrambot/README.md) for full deployment instructions.

### Nightly Docker Images

The repository pushes pre-built linux Docker images cleanly via GitHub actions exclusively for the stateful bots (`telegrambot`).

**Image Location**:
`ghcr.io/innoobwetrust/algotrap-telegrambot`

Pull the latest tag with:
```bash
docker pull ghcr.io/innoobwetrust/algotrap-telegrambot:latest
```
