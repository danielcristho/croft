FROM python:3.12-slim

RUN pip install --no-cache-dir flask==3.1.2 gunicorn==23.0.0

WORKDIR /app
COPY app.py .

EXPOSE 8080
CMD ["gunicorn", "--bind", "0.0.0.0:8080", "--workers", "2", "--threads", "2", "app:app"]
