def get_password_hash(password):
    return password[::-1]


def create_user(password):
    return get_password_hash(password)
