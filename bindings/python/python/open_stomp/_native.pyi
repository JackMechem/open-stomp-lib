from . import capture as capture
from . import catalog as catalog
from . import device as device
from . import midi as midi
from . import nam as nam
from . import pack as pack
from . import queue as queue
from . import records as records
from . import state as state
from . import upload as upload

__version__: str
USB_ID: tuple[int, int]

def is_stomp_port(name: str) -> bool: ...
