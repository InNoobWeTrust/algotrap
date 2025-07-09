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

## Deployment

This project supports various deployment methods: local development, Docker, and Kubernetes.

### Local Development

For local development, you can run the application directly using `Makefile.toml` and a `.env` file in the project root.

1.  **Create your `.env` file**: Ensure you have a `.env` file in the project root with all necessary environment variables. You can use `.env.example` as a reference.
2.  **Run with Makefile**: Use `cargo make` commands to run the application. For example:
    ```bash
    cargo make run
    ```
    Refer to `Makefile.toml` for available commands.

### Docker Usage

This section guides you on how to build and run the application using Docker locally, incorporating your local code changes and `.env` file.

#### Building the Docker Image Locally

To build the Docker images from your local codebase, navigate to the project root and run the following commands in sequence:

1.  **Build the base image**: This image contains common dependencies and is used as a base for other binaries.
    ```bash
    docker build -t algotrap-bins:latest -f Dockerfile.base .
    ```
2.  **Build the cryptobot image**: This image contains the `cryptobot` application.
    ```bash
    docker build -t algotrap-cryptobot:latest -f bins/cryptobot/deployment/Dockerfile bins/cryptobot
    ```

These commands build images tagged `algotrap-bins:latest` and `algotrap-cryptobot:latest` respectively.

#### Running the Docker Image Locally

To run the locally built `algotrap-cryptobot` Docker image with your local `.env` file, ensure you have a `.env` file in the `bins/cryptobot` directory and then execute from the project root:

```bash
if [ ! -f bins/cryptobot/.env ]; then \
  echo 'Error: .env file not found in bins/cryptobot/.'; \
  exit 1; \
fi; \
docker run --rm --env-file bins/cryptobot/.env algotrap-cryptobot
```


This command:
- Checks if a `.env` file exists in the current directory.
- Runs the `algotrap` Docker image.
- Mounts your local `.env` file into the container, making its environment variables available to the application.
- `--rm` ensures the container is removed after it exits.

### Docker Image on GitHub Container Registry

The project includes a GitHub Actions workflow to automatically build and push a Docker image to the GitHub Container Registry. This workflow runs nightly and can also be triggered manually.

**Image Location**:

The image for `cryptobot` is available at:
`ghcr.io/innoobwetrust/algotrap-cryptobot`

You can pull the latest image with:
```bash
docker pull ghcr.io/innoobwetrust/algotrap-cryptobot:latest
```

**Workflow Authentication**:

The GitHub Actions workflow uses the default `GITHUB_TOKEN` to authenticate with the GitHub Container Registry. The `permissions` for `packages: write` are set in the workflow file (`.github/workflows/nightly.yml`) to grant the necessary access. No further setup is required for the workflow to push images.

For users who wish to push images manually from their local machine, they will need to authenticate using a Personal Access Token (PAT) with the `write:packages` scope.

### Kubernetes Deployment (e.g., with OrbStack)

This project uses Kubernetes for deployment, supporting multiple cronjob configurations. Kubernetes secrets and cronjob definitions are generated from templates.

1.  **Create environment-specific `.env` files**: For each `cryptobot` cronjob instance you want to deploy, create a `.env` file in the `bins/cryptobot/deployment/envs/` directory (e.g., `bins/cryptobot/deployment/envs/ETH-USDT.env`). These files will contain the specific environment variables for each cronjob. You can use `bins/cryptobot/deployment/envs/ETH-USDT.env.example` as a reference.

2.  **Deploy Kubernetes cronjobs**: Run the cronjob deployment script from the `deployment` directory:
    ```bash
    ./deployment/deploy_cronjobs.sh
    ```
    This script iterates through the `.env` files in `deployment/env_configs/`, generates a `cronjob.yaml` and a symbol-specific `secret.yaml` for each, and applies them to your Kubernetes cluster. This script handles both secret and cronjob generation and deployment.

3.  **Apply other Kubernetes configurations**: If you have other Kubernetes configurations (e.g., deployments, services) besides the cronjobs handled by `deploy_cronjobs.sh`, you can apply them:
    ```bash
    kubectl apply -f k8s/
    ```
    If using OrbStack, ensure your Kubernetes cluster is running and configured correctly.
