FROM python:3.12-slim

WORKDIR /app

# Install system deps for Playwright (browsers)
RUN apt-get update && apt-get install -y \
    ca-certificates \
    curl \
    fonts-liberation \
    libasound2t64 \
    libatk-bridge2.0-0t64 \
    libatk1.0-0t64 \
    libcups2t64 \
    libdbus-1-3 \
    libdrm2 \
    libgbm1 \
    libglib2.0-0t64 \
    libnspr4 \
    libnss3 \
    libu2f-udev \
    libvulkan1 \
    libxcomposite1 \
    libxdamage1 \
    libxfixes3 \
    libxkbcommon0 \
    libxrandr2 \
    xdg-utils \
    && rm -rf /var/lib/apt/lists/*

# Install Python dependencies
COPY requirements.txt .
RUN pip install --no-cache-dir -r requirements.txt

# Install Playwright Chromium
RUN playwright install chromium

# Copy agent server
COPY server.py .
COPY agent.yaml .

ENV PLAYWRIGHT_BROWSERS_PATH=/root/.cache/ms-playwright
ENV BROWSER_AGENT_PORT=8083
ENV HEADLESS=true

EXPOSE 8083

CMD ["python", "-u", "server.py"]
