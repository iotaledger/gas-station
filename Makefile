


redis-start:
	docker run -d --name redis -p 6379:6379 redis:7.2.5

redis-restart:
	docker stop redis
	docker rm redis
	docker run -d --name redis -p 6379:6379 redis:7.2.5


