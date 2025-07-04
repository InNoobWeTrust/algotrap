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

## Deployment on Kubernetes (e.g., with OrbStack)

This project uses Kubernetes for deployment, with secrets managed via a templating script.

### Generating Kubernetes Secrets

The `k8s/secret.yaml` file is generated from `k8s/secret.yaml.template` using values from your `.env` file.

1.  **Create your `.env` file**: Ensure you have a `.env` file in the project root with all necessary environment variables. You can use `.env.example` as a reference.

2.  **Generate the secret**: Run the generation script from the `deployment` directory:
    ```bash
    ./deployment/generate_secret.sh
    ```
    This will create or update `k8s/secret.yaml` with your base64-encoded secret values.

3.  **Apply Kubernetes configurations**: Once `k8s/secret.yaml` is generated, you can apply your Kubernetes configurations:
    ```bash
    kubectl apply -f k8s/
    ```
    If using OrbStack, ensure your Kubernetes cluster is running and configured correctly.