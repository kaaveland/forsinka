FROM ubuntu:latest

ARG TARGETARCH

COPY ./forsinka-${TARGETARCH} /usr/local/bin/forsinka
RUN chmod +x /usr/local/bin/forsinka

RUN useradd -m -u 1000 forsinka
USER forsinka

ENTRYPOINT ["/usr/local/bin/forsinka"]