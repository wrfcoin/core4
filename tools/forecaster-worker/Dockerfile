# WRFCoin forecaster worker v0. Plain Python, no extra packages, no build step.
# python:3.12-slim-bookworm, pinned by digest.
FROM python@sha256:935c3c5b12357a7e10caa9c1e6ca5692344d97cf229832ef584be2cec9fc03df
ENV PYTHONUNBUFFERED=1 PYTHONDONTWRITEBYTECODE=1 WORKER_DATA_DIR=/data
WORKDIR /app
COPY forecaster_worker/ /app/forecaster_worker/
COPY README.md /app/README.md
# Runs as an ordinary user. The data folder (wallet and history) is mounted from the host.
USER 1000:1000
VOLUME ["/data"]
ENTRYPOINT ["python", "-m", "forecaster_worker"]
CMD ["run"]
