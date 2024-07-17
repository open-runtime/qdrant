import pytest

from .helpers.collection_setup import basic_collection_setup, drop_collection
from .helpers.helpers import request_with_validation

default_name = ""
collection_name = "collection_name"

@pytest.fixture(autouse=True)
def setup(on_disk_vectors):
    basic_collection_setup(
        collection_name=collection_name, on_disk_vectors=on_disk_vectors
    )
    yield
    drop_collection(collection_name=collection_name)


def test_collection_exists():
    response = request_with_validation(
        api="/collections/{collection_name}/exists",
        method="GET",
        path_params={"collection_name": collection_name},
    )
    assert response.ok
    result = response.json()["result"]["exists"]
    assert result


def test_collection_not_found():
    response = request_with_validation(
        api="/collections/{collection_name}/exists",
        method="GET",
        path_params={"collection_name": "wrong"},
    )
    assert response.ok
    result = response.json()["result"]["exists"]
    assert not result
