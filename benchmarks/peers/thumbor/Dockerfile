FROM python:3.13.18-slim-bookworm@sha256:a5a7017efc5969e00ae2b91c8eb0631049a569652aa4ffc4501769049ae45c72
WORKDIR /srv/thumbor
ENV PYTHONDONTWRITEBYTECODE=1
RUN apt-get update && apt-get install -y --no-install-recommends libcairo2 && rm -rf /var/lib/apt/lists/*
COPY requirements.txt .
RUN pip install --no-cache-dir -r requirements.txt
COPY thumbor.conf .
USER 65534:65534
EXPOSE 8080
CMD ["thumbor", "--ip=0.0.0.0", "--port=8080", "--conf=/srv/thumbor/thumbor.conf", "--log-level=warning"]
